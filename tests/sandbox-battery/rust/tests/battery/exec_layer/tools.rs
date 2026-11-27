use battery::Session;
use battery::adapter::{self, Tunables, path, sh_ok};
use battery::error::Res;
use battery::evidence::Checks;
use battery::ids::Lane;
use battery::proc::{Kube, LONG};
use br_agentic_harness_contract::{SandboxPort, Seed};
use br_runner_sandbox::{argv, argv::tar_create};

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
