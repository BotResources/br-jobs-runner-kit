use std::time::{Duration, Instant};

use battery::Session;
use battery::adapter::{self, request};
use battery::common::{record_stats, state_turn, timed_open, work_hash};
use battery::error::Res;
use battery::evidence::Checks;
use battery::ids::Lane;
use battery::prng::{Prng, Stats};
use battery::saves::Saves;
use battery::seeds;
use br_agentic_harness_contract::{FreshReason, Seed, StartKind};
use br_runner_sandbox::{Store, manifest_key};
use bytes::Bytes;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_save_never_stops_a_run() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    let set = seeds::standard()?;
    let lane = Lane::new()?;
    let (k1, _) = state_turn(&sandbox, &lane, None, 41, 30_000_000).await?;
    saves
        .delete_manifest_and_part(&saves.key(lane.thread, lane.agent, k1))
        .await?;
    let mut times = Vec::new();
    let mut verdicts = true;
    for round in 0..20 {
        let (_, turn) = lane.child(k1)?;
        let (millis, opened) = timed_open(&sandbox, turn, set.seed.clone()).await?;
        let named = matches!(
            opened.start,
            StartKind::Fresh(FreshReason::NoSaveEver | FreshReason::SaveExpired { .. })
        );
        verdicts &= named;
        if round == 0 {
            checks.expect(
                "missing-save-seed-files-are-present",
                seeds::present(&sandbox, &opened.box_id, &env.seed_root, &set).await?,
                format!("{:?}", opened.start),
            );
        }
        times.push(millis);
        adapter::release(&sandbox, &opened.box_id).await?;
        battery::pool::wait_warm(env, 1, 120).await?;
    }
    let stats = Stats::of(&times, 500.0);
    record_stats(env, "missing_save_start", &stats);
    checks.expect(
        "missing-save-open-is-ok-and-fresh-every-time",
        verdicts,
        "start kinds",
    );
    checks.expect(
        "missing-save-19-of-20-within-500-ms",
        stats.within >= 19,
        format!(
            "median {:.0}, slowest {:.0}, within {}",
            stats.median, stats.slowest, stats.within
        ),
    );
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_delete_during_the_restore_never_gives_an_error_or_a_half_restored_tree() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let saves = Saves::of(env)?;
    let mut checks = Checks::new(env);
    let lane = Lane::new()?;
    let (k1, _) = state_turn(&sandbox, &lane, None, 42, 100_000_000).await?;
    let key = saves.key(lane.thread, lane.agent, k1);
    let manifest = saves
        .store
        .get(&manifest_key(&key))
        .await?
        .ok_or_else(|| battery::error::data("manifest", "absent"))?;
    let part_key = saves.part_of(&key).await?;
    let part = saves
        .store
        .get(&part_key)
        .await?
        .ok_or_else(|| battery::error::data("part", "absent"))?;
    let (_, control_turn) = lane.child(k1)?;
    let control = Instant::now();
    let probe = adapter::open(&sandbox, control_turn, Seed::empty()).await?;
    let restore_ms = control.elapsed().as_millis() as u64;
    adapter::release(&sandbox, &probe.box_id).await?;
    let empty = {
        let (_, turn) = Lane::new()?.first()?;
        let blank = adapter::open(&sandbox, turn, Seed::empty()).await?;
        let hash = work_hash(&sandbox, &blank.box_id).await?;
        adapter::release(&sandbox, &blank.box_id).await?;
        hash
    };
    let (mut restored, mut fresh, mut errors, mut halves) = (0, 0, 0, 0);
    let mut prng = Prng::new(9);
    for round in 0..20u64 {
        saves.store.put(&part_key, Bytes::clone(&part)).await?;
        saves
            .store
            .put(&manifest_key(&key), Bytes::clone(&manifest))
            .await?;
        let offset = restore_ms * round / 19 + prng.below(20);
        let deleter = {
            let saves = Saves::of(env)?;
            let key = key.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(offset)).await;
                let _ = saves.store.delete(&manifest_key(&key)).await;
            })
        };
        let (_, turn) = lane.child(k1)?;
        match sandbox.open_box(&request(turn, Seed::empty(), None)?).await {
            Ok(opened) => {
                match opened.start {
                    StartKind::Restored { .. } => restored += 1,
                    StartKind::Fresh(_) => {
                        fresh += 1;
                        if work_hash(&sandbox, &opened.box_id).await? != empty {
                            halves += 1;
                        }
                    }
                    StartKind::Continued => errors += 1,
                }
                adapter::release(&sandbox, &opened.box_id).await?;
            }
            Err(_) => errors += 1,
        }
        let _ = deleter.await;
        battery::pool::wait_warm(env, 1, 120).await?;
    }
    env.record(
        "restore_race_results",
        format!("restored {restored}, fresh {fresh}, errors {errors}, half-restored {halves}"),
        "count",
    );
    checks.expect("restore-race-20-of-20-without-error-and-without-a-half-restored-tree", errors == 0 && halves == 0 && restored + fresh == 20, format!("restored {restored}, fresh {fresh}, errors {errors}, half {halves}, restore {restore_ms} ms"));
    checks.finish()
}
