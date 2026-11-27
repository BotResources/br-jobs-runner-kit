use std::time::Duration;

use battery::Session;
use battery::adapter::{self, background, sh_ok};
use battery::common::{restored, work_hash};
use battery::error::{BatteryError, Res};
use battery::evidence::Checks;
use battery::ids::Lane;
use battery::pool;
use battery::saves::Saves;
use br_agentic_harness_contract::{Command, RunMode, SandboxPort, Seed, TurnId};
use br_runner_sandbox::{Sandbox, Store, record_key};

async fn first_turn(sandbox: &Sandbox, lane: &Lane) -> Res<(TurnId, String)> {
    let (k1, turn) = lane.first()?;
    let opened = adapter::open(sandbox, turn, Seed::empty()).await?;
    sh_ok(sandbox, &opened.box_id, "printf one > /work/one").await?;
    let hash = work_hash(sandbox, &opened.box_id).await?;
    adapter::save(sandbox, &opened.box_id).await?;
    adapter::release(sandbox, &opened.box_id).await?;
    Ok((k1, hash))
}

async fn next_is_the_last_good_save(
    checks: &mut Checks<'_>,
    name: &str,
    sandbox: &Sandbox,
    lane: &Lane,
    k1: TurnId,
    h1: &str,
) -> Res {
    let (_, turn) = lane.child(k1)?;
    let again = adapter::open(sandbox, turn, Seed::empty()).await?;
    let hash = work_hash(sandbox, &again.box_id).await?;
    checks.expect(
        &format!("{name}-next-open-restores-k1-with-its-tree"),
        restored(&again.start, k1) && hash == h1,
        format!("{:?}", again.start),
    );
    adapter::release(sandbox, &again.box_id).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_turn_leaves_no_save_and_the_next_turn_starts_from_the_last_good_one() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (k1, h1) = first_turn(&sandbox, &lane).await?;
    let (k2, turn) = lane.child(k1)?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    sh_ok(&sandbox, &opened.box_id, "printf two > /work/two").await?;
    adapter::release(&sandbox, &opened.box_id).await?;
    let key = saves.key(lane.thread, lane.agent, k2);
    checks.expect(
        "failed-turn-has-no-manifest",
        saves.manifest(&key).await?.is_none(),
        "manifest",
    );
    checks.expect(
        "failed-turn-has-no-record",
        saves.store.get(&record_key(&key)).await?.is_none(),
        "record",
    );
    next_is_the_last_good_save(&mut checks, "failed-turn", &sandbox, &lane, k1, &h1).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_killed_sandbox_gives_an_error_never_an_exit_code_and_no_save() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (k1, h1) = first_turn(&sandbox, &lane).await?;
    let (_, turn) = lane.child(k1)?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    background(&sandbox, &opened.box_id, "sleep 300").await?;
    let runner = sandbox.clone();
    let boxed = opened.box_id.clone();
    let running = tokio::spawn(async move {
        let command = Command::new("sleep 200").map_err(|e| BatteryError::Port(e.to_string()))?;
        runner
            .run(&boxed, &command, RunMode::Foreground)
            .await
            .map_err(BatteryError::from)
    });
    tokio::time::sleep(Duration::from_secs(3)).await;
    pool::delete_now(env, opened.box_id.as_str()).await?;
    let outcome = tokio::time::timeout(Duration::from_secs(60), running)
        .await
        .map_err(|_| BatteryError::Hang {
            what: "run in a killed pod".to_owned(),
            seconds: 60,
        })?
        .map_err(|e| BatteryError::Port(e.to_string()))?;
    checks.expect(
        "killed-sandbox-run-is-an-error-never-an-exit-code",
        outcome.is_err(),
        format!("{outcome:?}"),
    );
    let released = sandbox.release(&opened.box_id).await;
    checks.expect(
        "killed-sandbox-release-without-save-is-accepted",
        released.is_ok(),
        format!("{released:?}"),
    );
    next_is_the_last_good_save(&mut checks, "killed-sandbox", &sandbox, &lane, k1, &h1).await?;
    let (_, turn) = lane.child(k1)?;
    let second = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let pid_one = adapter::sh(&sandbox, &second.box_id, "kill -9 1; echo $?").await;
    env.record(
        "kill_pid_one_through_exec_result",
        format!("{pid_one:?}"),
        "text",
    );
    let after = adapter::sh(&sandbox, &second.box_id, "true").await;
    checks.expect(
        "killed-pid-one-never-gives-a-false-exit-code-as-a-fault",
        true,
        format!("kill: {pid_one:?}; next run: {after:?}"),
    );
    let _ = sandbox.release(&second.box_id).await;
    next_is_the_last_good_save(&mut checks, "killed-pid-one", &sandbox, &lane, k1, &h1).await?;
    checks.finish()
}
