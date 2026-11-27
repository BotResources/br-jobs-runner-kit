use std::time::{Duration, Instant};

use battery::Session;
use battery::adapter::{self, Tunables, background, path, sh, sh_ok};
use battery::common::wait_until;
use battery::error::{BatteryError, Res};
use battery::evidence::Checks;
use battery::guards::PodReaper;
use battery::ids::Lane;
use battery::proc::{Kube, LONG, SHORT};
use battery::tunnel::{GateMode, api_gate};
use br_agentic_harness_contract::{Command, RunMode, SandboxPort, Seed};
use br_runner_sandbox::{argv, argv::tar_create};

#[tokio::test(flavor = "multi_thread")]
async fn exec_gives_true_exit_codes_and_ends_stdin_on_request() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let boxed = &opened.box_id;
    let zero = sh(&sandbox, boxed, "printf before; exit 0").await?;
    let seven = sh(&sandbox, boxed, "printf late; exit 7").await?;
    checks.expect(
        "exec-exit-code-zero",
        zero.exit_code == 0 && zero.stdout == b"before",
        format!("{:?}", zero.exit_code),
    );
    checks.expect(
        "exec-exit-code-seven-with-output-before-exit",
        seven.exit_code == 7 && seven.stdout == b"late",
        format!(
            "{} {:?}",
            seven.exit_code,
            String::from_utf8_lossy(&seven.stdout)
        ),
    );
    let exited = sh(&sandbox, boxed, "printf out; exit 137").await?;
    checks.expect(
        "exec-exit-code-137-of-a-command-in-a-live-pod-stays-an-exit-code",
        exited.exit_code == 137 && exited.stdout == b"out",
        format!(
            "{} {:?}",
            exited.exit_code,
            String::from_utf8_lossy(&exited.stdout)
        ),
    );
    let killed = sh(&sandbox, boxed, "printf out; kill -9 $$").await?;
    checks.expect(
        "exec-exit-code-137-of-a-killed-command-in-a-live-pod-stays-an-exit-code",
        killed.exit_code == 137 && killed.stdout == b"out",
        format!(
            "{} {:?}",
            killed.exit_code,
            String::from_utf8_lossy(&killed.stdout)
        ),
    );
    let content = battery::prng::Prng::new(3).bytes(5_000_000);
    let started = Instant::now();
    sandbox
        .write(boxed, &path("probe/blob.bin")?, content.clone())
        .await?;
    let back = sandbox.read(boxed, &path("probe/blob.bin")?).await?;
    checks.expect(
        "exec-write-ends-when-stdin-closes",
        back == content,
        format!("5 MB write and read in {:?}", started.elapsed()),
    );
    adapter::release(&sandbox, boxed).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_with_no_status_is_a_fault() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let (gate, context) = api_gate(env, "battery-exec-cut").await?;
    let mut tunables = Tunables::standard(env);
    tunables.context = context.name.clone();
    let sandbox = tunables.connect(env).await?;
    let mut checks = Checks::new(env);
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let pod = opened.box_id.to_string();
    let mut reaper = PodReaper::new(env);
    reaper.track(pod.clone());
    let runner = sandbox.clone();
    let boxed = opened.box_id.clone();
    let running = tokio::spawn(async move {
        let command = Command::new("sleep 120").map_err(|e| BatteryError::Port(e.to_string()))?;
        runner
            .run(&boxed, &command, RunMode::Foreground)
            .await
            .map_err(BatteryError::from)
    });
    tokio::time::sleep(Duration::from_secs(3)).await;
    gate.set(GateMode::Refuse);
    let outcome = tokio::time::timeout(Duration::from_secs(60), running)
        .await
        .map_err(|_| BatteryError::Hang {
            what: "running a cut stream".to_owned(),
            seconds: 60,
        })?
        .map_err(|e| BatteryError::Port(e.to_string()))?;
    let text = format!("{outcome:?}");
    checks.expect(
        "exec-stream-without-status-is-never-an-exit-code",
        outcome.is_err(),
        text.clone(),
    );
    checks.expect(
        "exec-no-status-cause-is-visible-through-the-port",
        text.contains("NoStatus") || text.to_lowercase().contains("no status"),
        text,
    );
    gate.set(GateMode::Open);
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn bsdtar_takes_the_argument_lists_of_the_crate() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let tunables = Tunables::standard(env);
    let config = tunables.config(env)?;
    let sandbox = tunables.connect(env).await?;
    let mut checks = Checks::new(env);
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let pod = opened.box_id.to_string();
    sh_ok(&sandbox, &opened.box_id, "printf data > /work/argv-probe").await?;
    let kube = Kube::of(env);
    let create = kube
        .exec(
            &pod,
            &env.agent_container,
            &tar_create(config.tools_path(), config.save_scope_paths()),
            None,
            LONG,
        )
        .await?;
    checks.expect(
        "bsdtar-create-accepts-no-xattrs-no-acls-no-fflags-and-hdrcharset",
        create.code == 0 && !create.stdout.is_empty(),
        format!("code {} stderr {}", create.code, create.stderr),
    );
    let archive = create.stdout;
    let restore = kube
        .exec(
            &pod,
            &env.agent_container,
            &argv::tar_restore_scoped(config.tools_path(), config.save_scope_paths()),
            Some(archive.clone()),
            LONG,
        )
        .await?;
    checks.expect(
        "bsdtar-restore-argument-list",
        restore.code == 0,
        format!("{} {}", restore.code, restore.stderr),
    );
    let write = kube
        .exec(
            &pod,
            &env.agent_container,
            &argv::tar_write(config.tools_path()),
            Some(archive.clone()),
            LONG,
        )
        .await?;
    checks.expect(
        "bsdtar-write-argument-list",
        write.code == 0,
        format!("{} {}", write.code, write.stderr),
    );
    let seed = kube
        .exec(
            &pod,
            &env.seed_container,
            &argv::tar_seed(config.seed_tools_path(), config.seed_root()),
            Some(archive),
            LONG,
        )
        .await?;
    checks.expect(
        "bsdtar-seed-argument-list",
        seed.code == 0,
        format!("{} {}", seed.code, seed.stderr),
    );
    adapter::release(&sandbox, &opened.box_id).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_background_command_survives_and_its_id_is_right() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let boxed = &opened.box_id;
    let process = background(&sandbox, boxed, "sleep 600").await?;
    let id = process.as_str().to_owned();
    let cmdline = sh_ok(
        &sandbox,
        boxed,
        &format!("tr '\\0' ' ' < /proc/{id}/cmdline"),
    )
    .await?;
    checks.expect(
        "background-id-is-the-id-of-the-command-not-of-the-shell",
        cmdline.trim() == "sleep 600",
        cmdline.clone(),
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    let alive = sh(&sandbox, boxed, &format!("[ -d /proc/{id} ]")).await?;
    checks.expect(
        "background-process-lives-after-the-exec-ended",
        alive.exit_code == 0,
        format!("{}", alive.exit_code),
    );
    sandbox.kill(boxed, &process).await?;
    wait_until("the killed process to be gone", 10, || async {
        Ok(sh(&sandbox, boxed, &format!("[ -d /proc/{id} ]"))
            .await?
            .exit_code
            != 0)
    })
    .await?;
    checks.expect("background-kill-ends-the-process", true, id);
    adapter::release(&sandbox, boxed).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_scripts_of_the_exec_layer_work_in_the_agent_image_shell() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let boxed = &opened.box_id;
    let missing = sandbox.list(boxed, Some(&path("no-such-folder")?)).await;
    checks.expect(
        "list-of-a-missing-folder-is-empty",
        matches!(&missing, Ok(list) if list.is_empty()),
        format!("{missing:?}"),
    );
    sh_ok(
        &sandbox,
        boxed,
        "printf x > \"/work/with\nnewline\"; printf y > /work/plain",
    )
    .await?;
    let listed = sandbox.list(boxed, None).await?;
    let names: Vec<String> = listed.iter().map(|p| p.to_string()).collect();
    checks.expect(
        "list-gives-names-with-a-newline",
        names.iter().any(|n| n == "with\nnewline") && names.iter().any(|n| n == "plain"),
        format!("{names:?}"),
    );
    let read_missing = sandbox.read(boxed, &path("absent.txt")?).await;
    checks.expect(
        "read-of-a-missing-file-is-a-refusal",
        matches!(&read_missing, Err(e) if e.is_refusal()),
        format!("{read_missing:?}"),
    );
    let read_plain = sandbox.read(boxed, &path("plain")?).await?;
    checks.expect(
        "read-gives-the-bytes",
        read_plain == b"y",
        format!("{read_plain:?}"),
    );
    adapter::release(&sandbox, boxed).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_over_exec_timeout_stops_near_the_limit() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut tunables = Tunables::standard(env);
    tunables.exec_timeout = Some(5);
    let sandbox = tunables.connect(env).await?;
    let mut checks = Checks::new(env);
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let started = Instant::now();
    let outcome = sh(&sandbox, &opened.box_id, "sleep 999").await;
    let seconds = started.elapsed().as_secs_f64();
    env.record("exec_timeout_return_seconds", format!("{seconds:.2}"), "s");
    checks.expect(
        "exec-timeout-returns-a-fault-near-the-limit",
        outcome.is_err() && (4.5..=8.0).contains(&seconds),
        format!("{seconds:.2} s, {outcome:?}"),
    );
    let _ = Kube::of(env)
        .try_run(&["get", "pod", opened.box_id.as_str()], None, SHORT)
        .await;
    adapter::release(&sandbox, &opened.box_id).await?;
    checks.finish()
}
