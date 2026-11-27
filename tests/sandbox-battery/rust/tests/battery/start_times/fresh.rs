use battery::Session;
use battery::adapter;
use battery::common::{ensure_floor, record_stats};
use battery::error::Res;
use battery::evidence::{Checks, load_now};
use battery::guards::ValueGuard;
use battery::ids::Lane;
use battery::prng::Stats;
use battery::seeds;

use super::bounded::bounded_fresh_series;
use super::support::{LIMIT_MS, fresh_series};

const STRESS_RUNS: usize = 20;

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
    let stats = Stats::of(&times, LIMIT_MS)?;
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
    let stress_series = bounded_fresh_series(
        env,
        "fresh_start_seed_50mb_500_files",
        &stress,
        STRESS_RUNS,
        3,
    )
    .await?;
    checks.expect(
        "fresh-start-stress-seed-every-open-is-accounted",
        stress_series.ended_ok + stress_series.stopped() == STRESS_RUNS,
        stress_series.summary(),
    );
    checks.expect(
        "fresh-start-stress-seed-has-at-least-one-sample",
        stress_series.ended_ok >= 1,
        stress_series.summary(),
    );
    if stress_series.times.is_empty() {
        return checks.finish();
    }
    let stress_stats = Stats::of(&stress_series.times, LIMIT_MS)?;
    record_stats(env, "fresh_start_seed_50mb_500_files", &stress_stats);
    checks.expect(
        "fresh-start-stress-seed-limit-reported-not-500-ms",
        stress_stats.count == stress_series.ended_ok,
        format!(
            "median {:.0}, slowest {:.0}; {}",
            stress_stats.median,
            stress_stats.slowest,
            stress_series.summary()
        ),
    );
    checks.finish()
}
