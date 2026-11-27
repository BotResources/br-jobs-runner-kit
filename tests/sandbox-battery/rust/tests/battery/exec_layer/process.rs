use std::time::{Duration, Instant};

use battery::Session;
use battery::adapter::{self, Tunables, background, path, sh, sh_ok};
use battery::common::wait_until;
use battery::error::Res;
use battery::evidence::Checks;
use battery::ids::Lane;
use br_agentic_harness_contract::{SandboxPort, Seed};

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
    let ended = wait_until("the killed process to be gone", 10, || async {
        Ok(sh(&sandbox, boxed, &format!("[ -d /proc/{id} ]"))
            .await?
            .exit_code
            != 0)
    })
    .await;
    checks.expect(
        "background-kill-ends-the-process",
        ended.is_ok(),
        format!("{id}: {ended:?}"),
    );
    adapter::release(&sandbox, boxed).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_over_exec_timeout_returns_a_fault_near_the_limit() -> Res {
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
    let remaining = sh(
        &sandbox,
        &opened.box_id,
        "for p in /proc/[0-9]*; do tr '\\0' ' ' < $p/cmdline 2>/dev/null; echo; done | grep -c 'sleep 99[9]'",
    )
    .await;
    env.record(
        "exec_timeout_command_still_in_the_pod_after_the_return",
        format!("{remaining:?}"),
        "text",
    );
    adapter::release(&sandbox, &opened.box_id).await?;
    checks.finish()
}
