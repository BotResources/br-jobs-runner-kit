use std::time::Instant;

use battery::Session;
use battery::adapter;
use battery::common::{ensure_floor, record_stats, restored, timed_open};
use battery::error::{Res, data};
use battery::evidence::Checks;
use battery::guards::ValueGuard;
use battery::ids::Lane;
use battery::prng::Stats;
use battery::seeds;
use br_agentic_harness_contract::Seed;

use super::support::{LIMIT_MS, fresh_series, ladder_save};

const STALL_MS: f64 = 3000.0;
const SAVES_PER_SIZE: usize = 3;

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
            let saved = ladder_save(env, &sandbox, &candidate, megabytes).await?;
            let (id, expected) = (saved.id, saved.expected_raw_bytes);
            match saved.outcome {
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
                            "ok, raw bytes {}, scope file bytes {}",
                            receipt.bytes, saved.scope_file_bytes
                        ),
                    );
                    checks.expect(
                        &format!("ladder-save-{megabytes}mb-{attempt}-size-is-the-expected-size"),
                        receipt.bytes == expected,
                        format!("save-bytes got={} expected={expected}", receipt.bytes),
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
