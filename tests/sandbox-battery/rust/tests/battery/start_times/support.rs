use std::time::Duration;

use battery::adapter::{self, Tunables, sh_ok};
use battery::common::timed_open;
use battery::error::Res;
use battery::ids::Lane;
use battery::seeds;
use br_agentic_harness_contract::{SaveReceipt, Seed, TurnId};
use br_runner_sandbox::Sandbox;

pub(super) const LIMIT_MS: f64 = 500.0;
const SEED_EXEC_BOUND_SECONDS: u64 = 120;
const OPEN_OUTER_BOUND_SECONDS: u64 = 300;
const SCOPE_BYTES: &str =
    "find /work /home/agent -type f -printf '%s\\n' | awk '{s+=$1} END{print s+0}'";

pub(super) async fn ladder_save(
    sandbox: &Sandbox,
    lane: &Lane,
    megabytes: u64,
) -> Res<(TurnId, Result<SaveReceipt, String>, String)> {
    let (id, turn) = lane.first()?;
    let opened = adapter::open(sandbox, turn, Seed::empty()).await?;
    battery::tree::state(sandbox, &opened.box_id, megabytes, megabytes * 1_000_000).await?;
    let expected = sh_ok(sandbox, &opened.box_id, SCOPE_BYTES).await?;
    let outcome = sandbox
        .save_box(&opened.box_id)
        .await
        .map_err(|error| error.to_string());
    adapter::release(sandbox, &opened.box_id).await?;
    Ok((id, outcome, expected.trim().to_owned()))
}

pub(super) async fn fresh_series(
    env: &battery::env::Env,
    set: &seeds::SeedSet,
    runs: usize,
    floor: usize,
) -> Res<Vec<f64>> {
    let sandbox = adapter::standard(env).await?;
    let mut times = Vec::new();
    for _ in 0..runs {
        let (millis, opened) =
            timed_open(&sandbox, Lane::new()?.first()?.1, set.seed.clone()).await?;
        times.push(millis);
        adapter::release(&sandbox, &opened.box_id).await?;
        battery::pool::wait_warm(env, floor, 120).await?;
    }
    Ok(times)
}

pub(super) async fn bounded_fresh_series(
    env: &battery::env::Env,
    name: &str,
    set: &seeds::SeedSet,
    runs: usize,
    floor: usize,
) -> Res<(Vec<f64>, usize)> {
    let mut tunables = Tunables::standard(env);
    tunables.exec_timeout = Some(SEED_EXEC_BOUND_SECONDS);
    let sandbox = tunables.connect(env).await?;
    let mut times = Vec::new();
    let mut fired = 0;
    for run in 1..=runs {
        let lane = Lane::new()?;
        let outcome = tokio::time::timeout(
            Duration::from_secs(OPEN_OUTER_BOUND_SECONDS),
            timed_open(&sandbox, lane.first()?.1, set.seed.clone()),
        )
        .await;
        match outcome {
            Ok(result) => {
                let (millis, opened) = result?;
                times.push(millis);
                adapter::release(&sandbox, &opened.box_id).await?;
            }
            Err(_) => {
                fired += 1;
                let thread = lane.thread.to_string();
                let pods = battery::pool::pods_of_thread(env, &thread).await?;
                for pod in &pods {
                    battery::pool::delete_now(env, pod).await?;
                }
                env.record(
                    &format!("{name}_run_{run}_outer_bound_fired"),
                    format!(
                        "the open did not return in {OPEN_OUTER_BOUND_SECONDS} s; pods deleted {}",
                        pods.join(",")
                    ),
                    "event",
                );
            }
        }
        battery::pool::wait_warm(env, floor, 180).await?;
    }
    Ok((times, fired))
}
