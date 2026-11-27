use std::time::{Duration, Instant};

use battery::Session;
use battery::adapter::{self, Tunables, sh_ok};
use battery::common::{restored, wait_until};
use battery::error::Res;
use battery::evidence::Checks;
use battery::ids::Lane;
use battery::pool;
use battery::saves::Saves;
use battery::seeds;
use battery::tree;
use br_agentic_harness_contract::{SandboxPort, Seed};
use br_runner_sandbox::{
    ANNOTATION_SEED_HASH, Error, ExecError, Record, RecordError, RecordState, Store, record_key,
};

const SCOPE_SETUP: &str = "mkdir -p /work/empty-dir /work/sub && printf secret > /work/private && chmod 0600 /work/private && printf run > /work/sub/tool.sh && chmod 0755 /work/sub/tool.sh && ln -s sub/tool.sh /work/link && printf data > \"/work/$(printf 'bad\\377name')\" && printf home > /home/agent/home-file && touch -d '@1700000123' /work/sub/tool.sh /work/private";

#[tokio::test(flavor = "multi_thread")]
async fn save_then_restore_gives_back_files_modes_owners_dirs_and_times() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (first, turn) = lane.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    sh_ok(&sandbox, &opened.box_id, SCOPE_SETUP).await?;
    let before = tree::capture(&sandbox, &opened.box_id).await?;
    adapter::save(&sandbox, &opened.box_id).await?;
    adapter::release(&sandbox, &opened.box_id).await?;
    let (_, next) = lane.child(first)?;
    let again = adapter::open(&sandbox, next, Seed::empty()).await?;
    checks.expect(
        "restore-start-is-restored",
        restored(&again.start, first),
        format!("{:?}", again.start),
    );
    let after = tree::capture(&sandbox, &again.box_id).await?;
    let differences = tree::diff(&before, &after, 1);
    checks.expect(
        "save-restore-tree-equal",
        differences.is_empty(),
        differences.join(" | "),
    );
    let kinds: Vec<&str> = before.entries.values().map(|e| e.kind.as_str()).collect();
    checks.expect(
        "save-restore-case-holds-symlink-and-empty-directory",
        kinds.contains(&"l") && kinds.contains(&"d"),
        format!("{} entries", kinds.len()),
    );
    adapter::release(&sandbox, &again.box_id).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_user_cannot_write_files_owned_by_another_user() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    sandbox
        .write(
            &opened.box_id,
            &adapter::path("owned.txt")?,
            b"mine".to_vec(),
        )
        .await?;
    let owner = sh_ok(
        &sandbox,
        &opened.box_id,
        "[ \"$(stat -c %u /work/owned.txt)\" = \"$(id -u)\" ] && echo same",
    )
    .await?;
    checks.expect("write-uses-no-same-owner", owner.trim() == "same", owner);
    adapter::release(&sandbox, &opened.box_id).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_save_of_a_missing_scope_path_is_a_fault_with_the_record_failed() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut tunables = Tunables::standard(env);
    tunables.scope = vec!["/work".to_owned(), "/home/agent-missing".to_owned()];
    let sandbox = tunables.connect(env).await?;
    let saves = Saves::through(env, &tunables)?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (id, turn) = lane.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let outcome = sandbox.save_box(&opened.box_id).await;
    let exec_fault = matches!(&outcome, Err(Error::Exec(_)));
    checks.expect(
        "save-missing-scope-is-an-exec-fault",
        exec_fault,
        format!("{outcome:?}"),
    );
    let key = saves.key(lane.thread, lane.agent, id);
    let bytes = saves.store.get(&record_key(&key)).await?;
    let record = bytes.map(|b| Record::parse(&b));
    let failed_exec = matches!(
        &record,
        Some(Ok(r)) if matches!(r.state(), RecordState::Failed { error: RecordError::Exec, .. })
    );
    checks.expect(
        "save-missing-scope-record-is-failed-exec",
        failed_exec,
        format!("{record:?}"),
    );
    adapter::release(&sandbox, &opened.box_id).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_save_over_save_timeout_stops_with_the_record_timeout() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut tunables = Tunables::standard(env);
    tunables.save_timeout = Some(2);
    let sandbox = tunables.connect(env).await?;
    let saves = Saves::through(env, &tunables)?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (id, turn) = lane.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    sh_ok(
        &sandbox,
        &opened.box_id,
        "head -c 400000000 /dev/urandom > /work/large.bin",
    )
    .await?;
    let started = Instant::now();
    let outcome = sandbox.save_box(&opened.box_id).await;
    let seconds = started.elapsed().as_secs_f64();
    env.record("save_timeout_return_seconds", format!("{seconds:.2}"), "s");
    checks.expect(
        "save-timeout-returns-near-the-limit-as-timed-out",
        matches!(&outcome, Err(Error::Exec(ExecError::TimedOut { .. }))) && seconds < 6.0,
        format!("{seconds:.2} s, {outcome:?}"),
    );
    let key = saves.key(lane.thread, lane.agent, id);
    let record = saves
        .store
        .get(&record_key(&key))
        .await?
        .map(|b| Record::parse(&b));
    let timed = matches!(
        &record,
        Some(Ok(r)) if matches!(r.state(), RecordState::Failed { error: RecordError::Timeout, .. })
    );
    checks.expect(
        "save-timeout-record-is-failed-timeout",
        timed,
        format!("{record:?}"),
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    let left = sh_ok(&sandbox, &opened.box_id, "for f in /proc/[0-9]*/cmdline; do tr '\\0' ' ' < $f; echo; done 2>/dev/null | grep -c '[b]sdtar --create' || true").await?;
    checks.expect(
        "save-timeout-leaves-no-bsdtar-in-the-pod",
        left.trim() == "0",
        left,
    );
    adapter::release(&sandbox, &opened.box_id).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_seed_is_root_owned_and_read_only_for_the_agent() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let set = seeds::make(21, 2, 1000, 500)?;
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, set.seed.clone()).await?;
    let boxed = &opened.box_id;
    let root = &env.seed_root;
    let owners = sh_ok(
        &sandbox,
        boxed,
        &format!("find {root} -type f -exec stat -c '%u:%g %a' {{}} +"),
    )
    .await?;
    let all_root = !owners.is_empty()
        && owners.lines().all(|l| {
            l.starts_with("0:0 ")
                && !l
                    .split(' ')
                    .nth(1)
                    .unwrap_or("")
                    .chars()
                    .any(|c| matches!(c, '2' | '3' | '6' | '7'))
        });
    checks.expect("seed-files-are-0-0-with-no-write-bit", all_root, owners);
    let refused = adapter::sh(
        &sandbox,
        boxed,
        &format!("touch {root}/agent-file 2>/dev/null"),
    )
    .await?;
    let refused_file = adapter::sh(
        &sandbox,
        boxed,
        &format!("printf x >> {root}/f0.bin 2>/dev/null"),
    )
    .await?;
    checks.expect(
        "seed-write-from-the-agent-container-is-refused",
        refused.exit_code != 0 && refused_file.exit_code != 0,
        format!("{} {}", refused.exit_code, refused_file.exit_code),
    );
    checks.expect(
        "seed-content-is-the-seed",
        seeds::present(&sandbox, boxed, root, &set).await?,
        "hash compare",
    );
    let pod = pool::pod(env, boxed.as_str()).await?;
    let annotation = pod
        .as_ref()
        .and_then(|p| pool::annotation(p, ANNOTATION_SEED_HASH));
    checks.expect(
        "seed-hash-annotation-is-on-the-pod",
        annotation.is_some(),
        format!("{annotation:?}"),
    );
    adapter::release(&sandbox, boxed).await?;
    wait_until("the seed pod to be gone", 60, || async {
        Ok(pool::pod(env, boxed.as_str()).await?.is_none())
    })
    .await?;
    checks.finish()
}
