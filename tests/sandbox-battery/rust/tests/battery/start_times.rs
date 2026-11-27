use std::time::Instant;

use battery::Session;
use battery::adapter::{self, Tunables, sh_ok};
use battery::common::{ensure_floor, record_stats, restored, timed_open};
use battery::error::{Res, data};
use battery::evidence::{Checks, load_now};
use battery::guards::{ScaleGuard, ValueGuard};
use battery::ids::Lane;
use battery::prng::Stats;
use battery::seeds;
use br_agentic_harness_contract::{SaveReceipt, Seed, TurnId};
use br_runner_sandbox::Sandbox;
use br_runner_sandbox::Timestamp;

const LIMIT_MS: f64 = 500.0;
const STALL_MS: f64 = 3000.0;
const SAVES_PER_SIZE: usize = 3;
const SCOPE_BYTES: &str =
    "find /work /home/agent -type f -printf '%s\\n' | awk '{s+=$1} END{print s+0}'";

async fn ladder_save(
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

async fn fresh_series(
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fresh_start_with_a_seed_of_10_mb_is_within_500_ms() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut values = ValueGuard::new(env);
    ensure_floor(env, &mut values, 3).await?;
    let mut checks = Checks::new(env);
    let set = seeds::standard()?;
    let sandbox = adapter::standard(env).await?;
    let probe = adapter::open(&sandbox, Lane::new()?.first()?.1, set.seed.clone()).await?;
    checks.expect(
        "fresh-start-seed-files-are-present",
        seeds::present(&sandbox, &probe.box_id, &env.seed_root, &set).await?,
        "hash compare",
    );
    adapter::release(&sandbox, &probe.box_id).await?;
    battery::pool::wait_warm(env, 3, 120).await?;
    let times = fresh_series(env, &set, 20, 3).await?;
    let stats = Stats::of(&times, LIMIT_MS);
    record_stats(env, "fresh_start_seed_10mb", &stats);
    checks.expect(
        "fresh-start-19-of-20-within-500-ms",
        stats.within >= 19,
        format!(
            "median {:.0}, p95 {:.0}, slowest {:.0}, within {} of 20, load {:?}",
            stats.median,
            stats.p95,
            stats.slowest,
            stats.within,
            load_now()
        ),
    );
    let stress = seeds::stress()?;
    let stress_times = fresh_series(env, &stress, 20, 3).await?;
    let stress_stats = Stats::of(&stress_times, LIMIT_MS);
    record_stats(env, "fresh_start_seed_50mb_500_files", &stress_stats);
    checks.expect(
        "fresh-start-stress-seed-recorded",
        true,
        format!(
            "limit reported, not 500 ms: median {:.0}, slowest {:.0}",
            stress_stats.median, stress_stats.slowest
        ),
    );
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restored_start_ladder_10_30_50_100_mb_with_stalls_and_split() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let mut values = ValueGuard::new(env);
    ensure_floor(env, &mut values, 4).await?;
    let mut checks = Checks::new(env);
    let set = seeds::standard()?;
    let sandbox = adapter::standard(env).await?;
    let claim_times = fresh_series(env, &seeds::make(1, 0, 0, 0)?, 10, 4).await?;
    let claim = Stats::of(&claim_times, LIMIT_MS);
    env.record(
        "split_claim_and_first_true_median",
        format!("{:.0}", claim.median),
        "ms",
    );
    let seed_times = fresh_series(env, &set, 10, 4).await?;
    let seeded = Stats::of(&seed_times, LIMIT_MS);
    env.record(
        "split_seed_copy_in_median",
        format!("{:.0}", seeded.median - claim.median),
        "ms",
    );
    for megabytes in [10u64, 30, 50, 100] {
        let mut failed = 0usize;
        let mut errors = Vec::new();
        let mut chosen = None;
        for attempt in 1..=SAVES_PER_SIZE {
            let candidate = Lane::new()?;
            let (id, outcome, expected) = ladder_save(&sandbox, &candidate, megabytes).await?;
            match outcome {
                Ok(receipt) => {
                    env.record(
                        &format!("save_state_{megabytes}mb_raw_bytes"),
                        receipt.bytes,
                        "bytes",
                    );
                    checks.expect(
                        &format!("ladder-save-{megabytes}mb-{attempt}-is-ok"),
                        true,
                        format!(
                            "ok, raw bytes {}, scope file bytes {expected}",
                            receipt.bytes
                        ),
                    );
                    if chosen.is_none() {
                        chosen = Some((candidate, id));
                    }
                }
                Err(text) => {
                    failed += 1;
                    errors.push(text.clone());
                    checks.expect(
                        &format!("ladder-save-{megabytes}mb-{attempt}-is-ok"),
                        false,
                        format!(
                            "save-bytes got=unknown expected={expected} (the error carries no byte count); {text}"
                        ),
                    );
                }
            }
        }
        env.record(
            &format!("ladder_saves_{megabytes}mb_made_failed"),
            format!("{SAVES_PER_SIZE} made, {failed} failed, errors {errors:?}"),
            "count",
        );
        let Some((lane, state)) = chosen else {
            continue;
        };
        let mut times = Vec::new();
        let mut stalls = Vec::new();
        for _ in 0..20 {
            let (_, turn) = lane.child(state)?;
            let (millis, opened) = timed_open(&sandbox, turn, set.seed.clone()).await?;
            if !restored(&opened.start, state) {
                return Err(data(
                    "restore ladder",
                    format!("{megabytes} MB start was {:?}", opened.start),
                ));
            }
            if millis > STALL_MS {
                stalls.push(millis);
            }
            times.push(millis);
            adapter::release(&sandbox, &opened.box_id).await?;
            battery::pool::wait_warm(env, 4, 120).await?;
        }
        let stats = Stats::of(&times, LIMIT_MS);
        record_stats(env, &format!("restored_start_{megabytes}mb"), &stats);
        env.record(
            &format!("restore_stalls_{megabytes}mb_count"),
            stalls.len(),
            &format!("over {STALL_MS:.0} ms"),
        );
        env.record(
            &format!("restore_stalls_{megabytes}mb_durations"),
            format!("{stalls:?}"),
            "ms",
        );
        let mut only_restore = Vec::new();
        for _ in 0..10 {
            let (_, turn) = lane.child(state)?;
            let (millis, opened) = timed_open(&sandbox, turn, Seed::empty()).await?;
            only_restore.push(millis);
            adapter::release(&sandbox, &opened.box_id).await?;
            battery::pool::wait_warm(env, 4, 120).await?;
        }
        let split = Stats::of(&only_restore, LIMIT_MS);
        env.record(
            &format!("split_restore_stream_{megabytes}mb_median"),
            format!("{:.0}", split.median - claim.median),
            "ms",
        );
        checks.expect(
            &format!("restored-start-{megabytes}mb-19-of-20-within-500-ms"),
            stats.within >= 19,
            format!(
                "median {:.0}, p95 {:.0}, slowest {:.0}, within {} of 20, stalls {}",
                stats.median,
                stats.p95,
                stats.slowest,
                stats.within,
                stalls.len()
            ),
        );
        if megabytes == 30 {
            let started = Instant::now();
            let runs = futures::future::join_all((0..4).map(|_| {
                let sandbox = sandbox.clone();
                let seed = set.seed.clone();
                let turn = lane.child(state).map(|c| c.1);
                async move { timed_open(&sandbox, turn?, seed).await }
            }))
            .await;
            let mut each = Vec::new();
            for run in runs {
                let (millis, opened) = run?;
                each.push(millis);
                adapter::release(&sandbox, &opened.box_id).await?;
            }
            env.record(
                "restored_start_30mb_four_at_once_total",
                format!("{:.0}", started.elapsed().as_secs_f64() * 1000.0),
                "ms",
            );
            env.record(
                "restored_start_30mb_four_at_once_each",
                format!("{each:?}"),
                "ms",
            );
            battery::pool::wait_warm(env, 4, 120).await?;
        }
    }
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cold_start_effect_of_the_init_container_and_the_image_size() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let keeper = ScaleGuard::scale_keeper(env, 0).await?;
    battery::common::scale_pool(env, 0).await?;
    battery::common::wait_until("no warm pod", 120, || async {
        Ok(battery::pool::warm_list(env, false).await?.is_empty())
    })
    .await?;
    let mut checks = Checks::new(env);
    let sandbox = Tunables::standard(env).connect(env).await?;
    let mut ready_times = Vec::new();
    let mut init_seconds = Vec::new();
    let mut image = String::new();
    for _ in 0..5 {
        let (millis, opened) = timed_open(&sandbox, Lane::new()?.first()?.1, Seed::empty()).await?;
        ready_times.push(millis);
        if let Some(pod) = battery::pool::pod(env, opened.box_id.as_str()).await? {
            let status = &pod["status"]["initContainerStatuses"][0]["state"]["terminated"];
            if let (Some(a), Some(b)) =
                (status["startedAt"].as_str(), status["finishedAt"].as_str())
                && let (Ok(a), Ok(b)) = (Timestamp::parse(a), Timestamp::parse(b))
            {
                let span = b
                    .to_system_time()
                    .duration_since(a.to_system_time())
                    .unwrap_or_default();
                init_seconds.push(span.as_secs_f64() * 1000.0);
            }
            image = pod["spec"]["containers"][0]["image"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
        }
        adapter::release(&sandbox, &opened.box_id).await?;
        battery::pool::wait_gone_all(env).await?;
    }
    let stats = Stats::of(&ready_times, LIMIT_MS);
    record_stats(env, "cold_start_to_first_true", &stats);
    let init = Stats::of(&init_seconds, LIMIT_MS);
    env.record(
        "cold_start_init_container_median",
        format!("{:.0}", init.median),
        "ms",
    );
    let sizes = battery::pool::node_exec(env, "k3s crictl images -o json").await?;
    let parsed: serde_json::Value = serde_json::from_slice(&sizes.stdout).unwrap_or_default();
    let bytes = parsed["images"]
        .as_array()
        .and_then(|all| {
            all.iter().find(|i| {
                i["repoTags"].as_array().is_some_and(|t| {
                    t.iter().any(|x| {
                        x.as_str()
                            .is_some_and(|tag| tag == image || tag.ends_with(&format!("/{image}")))
                    })
                })
            })
        })
        .and_then(|i| i["size"].as_str().and_then(|s| s.parse::<u64>().ok()));
    env.record(
        "cold_start_image_size",
        bytes.map_or_else(|| "unknown".to_owned(), |b| b.to_string()),
        "bytes",
    );
    checks.expect(
        "cold-start-measured-with-init-container-time-and-image-size",
        !init_seconds.is_empty() && bytes.is_some(),
        format!(
            "init samples {}, image {image}, size {bytes:?}",
            init_seconds.len()
        ),
    );
    drop(keeper);
    checks.finish()
}
