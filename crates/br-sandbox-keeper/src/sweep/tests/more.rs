use super::*;
use crate::bucket::ObjectInfo;

#[tokio::test(start_paused = true)]
async fn a_failed_step_is_counted_and_the_next_sweep_retries_it() {
    let harness = harness(true, 100);
    let key = save_key();
    let attempt = AttemptId::mint();
    put_save(&harness.bucket, &key, &[attempt], attempt, NOW - 5000);
    harness.bucket.fail_delete(&manifest_key(&key));
    harness.sweeper.sweep_once().await;
    assert!(metric(&harness, "br_sandbox_keeper_sweep_errors_total 1"));
    assert!(harness.bucket.contains(&tombstone_key(&key)));
    assert!(harness.bucket.contains(&part_object(&key, attempt)));
    let healed = Arc::new(FakeBucket::default());
    for name in harness.bucket.keys() {
        healed.insert(&name, &harness.bucket.body(&name).unwrap(), NOW - 5000);
    }
    let again = Sweeper {
        bucket: healed.clone(),
        ..harness_sweeper(&harness)
    };
    again.sweep_once().await;
    assert!(!healed.contains(&manifest_key(&key)));
    assert!(!healed.contains(&part_object(&key, attempt)));
}

#[tokio::test(start_paused = true)]
async fn the_listing_is_paged_and_every_turn_is_swept() {
    let harness = harness(true, 2);
    let mut keys = Vec::new();
    for _ in 0..5 {
        let key = save_key();
        let attempt = AttemptId::mint();
        put_save(&harness.bucket, &key, &[attempt], attempt, NOW - 5000);
        keys.push(key);
    }
    harness.sweeper.sweep_once().await;
    for key in &keys {
        assert!(harness.bucket.contains(&tombstone_key(key)));
        assert!(!harness.bucket.contains(&manifest_key(key)));
    }
    let calls = harness.bucket.list_calls();
    assert!(calls.iter().all(|(_, _, limit)| *limit == 2));
    assert!(
        calls
            .iter()
            .filter(|(prefix, _, _)| prefix == "saves/")
            .count()
            > 2
    );
}

#[tokio::test(start_paused = true)]
async fn the_keeper_puts_only_under_tombstones() {
    let harness = harness(true, 100);
    let key = save_key();
    let attempt = AttemptId::mint();
    put_save(&harness.bucket, &key, &[attempt], attempt, NOW - 5000);
    harness.sweeper.sweep_once().await;
    assert!(
        harness
            .bucket
            .writes()
            .iter()
            .filter(|line| line.starts_with("put "))
            .all(|line| line.starts_with("put tombstones/"))
    );
}

#[tokio::test(start_paused = true)]
async fn the_run_loop_sweeps_at_each_interval_and_stops_at_shutdown() {
    let harness = harness(true, 100);
    let shutdown = CancellationToken::new();
    let bucket = harness.bucket.clone();
    let metrics = harness.sweeper.metrics.clone();
    let handle = tokio::spawn(harness.sweeper.run(shutdown.clone()));
    tokio::time::sleep(Duration::from_secs(1)).await;
    let first = bucket.list_calls().len();
    assert!(first > 0);
    tokio::time::sleep(Duration::from_secs(61)).await;
    assert!(bucket.list_calls().len() > first);
    shutdown.cancel();
    handle.await.unwrap();
    drop(metrics);
}

#[tokio::test(start_paused = true)]
async fn a_manifest_that_cannot_be_fetched_is_counted_and_left_in_place() {
    let harness = harness(true, 100);
    let key = save_key();
    let attempt = AttemptId::mint();
    put_save(&harness.bucket, &key, &[attempt], attempt, NOW - 5000);
    harness.bucket.fail_get(&manifest_key(&key));
    harness.sweeper.sweep_once().await;
    assert!(metric(&harness, "br_sandbox_keeper_sweep_errors_total 1"));
    assert!(harness.bucket.writes().is_empty());
    assert!(harness.bucket.contains(&manifest_key(&key)));
}

#[tokio::test(start_paused = true)]
async fn a_manifest_that_cannot_be_read_is_logged_with_its_key_counted_and_never_shown() {
    let (logs, _guard) = crate::testing::logs::capture();
    let harness = harness(true, 100);
    let (garbled, foreign, stray) = (save_key(), save_key(), save_key());
    let attempt = AttemptId::mint();
    harness.bucket.insert(
        &manifest_key(&garbled),
        br#"{"made_at":"SECRET-CONTENT-1"}"#,
        10,
    );
    let other = manifest(&stray, attempt, at(10));
    harness
        .bucket
        .insert(&manifest_key(&foreign), &other.to_vec().unwrap(), 10);
    harness.sweeper.sweep_once().await;
    assert!(metric(&harness, "br_sandbox_keeper_sweep_errors_total 2"));
    assert!(harness.bucket.writes().is_empty());
    let text = logs.text();
    assert!(text.contains(&manifest_key(&garbled)));
    assert!(text.contains(&manifest_key(&foreign)));
    assert!(!text.contains("SECRET-CONTENT-1"));
}

#[tokio::test(start_paused = true)]
async fn the_logs_of_a_sweep_hold_the_key_of_an_object_and_never_its_content() {
    let (logs, _guard) = crate::testing::logs::capture();
    let harness = harness(true, 100);
    let key = save_key();
    harness
        .bucket
        .insert(&record_key(&key), br#"{"state":"SECRET-CONTENT-2"}"#, 10);
    harness
        .bucket
        .insert(&tombstone_key(&key), br#"{"turn":"SECRET-CONTENT-3"}"#, 10);
    harness.sweeper.sweep_once().await;
    let text = logs.text();
    assert!(text.contains(&record_key(&key)));
    assert!(text.contains(&tombstone_key(&key)));
    assert!(!text.contains("SECRET-CONTENT"));
}

struct LeaseRunsOutAfterAWrite {
    inner: Arc<FakeBucket>,
}

impl crate::sealed::Sealed for LeaseRunsOutAfterAWrite {}

impl Bucket for LeaseRunsOutAfterAWrite {
    fn check(&self) -> crate::bucket::BucketFuture<'_, ()> {
        self.inner.check()
    }

    fn list_page<'a>(
        &'a self,
        prefix: &'a str,
        after: Option<&'a str>,
        limit: usize,
    ) -> crate::bucket::BucketFuture<'a, Vec<ObjectInfo>> {
        self.inner.list_page(prefix, after, limit)
    }

    fn get<'a>(&'a self, key: &'a str) -> crate::bucket::BucketFuture<'a, Option<Bytes>> {
        self.inner.get(key)
    }

    fn put<'a>(&'a self, key: &'a str, body: Bytes) -> crate::bucket::BucketFuture<'a, ()> {
        Box::pin(async move {
            self.inner.put(key, body).await?;
            tokio::time::advance(Duration::from_secs(10)).await;
            Ok(())
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> crate::bucket::BucketFuture<'a, ()> {
        Box::pin(async move {
            self.inner.delete(key).await?;
            tokio::time::advance(Duration::from_secs(10)).await;
            Ok(())
        })
    }
}

fn leader_for_five_seconds() -> (Arc<FakeBucket>, Sweeper) {
    let bucket = Arc::new(FakeBucket::default());
    let clock: Arc<dyn Clock> = Arc::new(TokioClock::starting_at(at(NOW)));
    let leadership = Leadership::new(clock.clone());
    leadership.renewed(at(NOW + 5));
    let sweeper = Sweeper {
        bucket: Arc::new(GuardedBucket::new(Arc::new(LeaseRunsOutAfterAWrite {
            inner: bucket.clone(),
        }))),
        clock,
        leadership,
        metrics: Arc::new(Metrics::new()),
        rules: rules(),
        page_size: 100,
        interval: Duration::from_secs(60),
    };
    (bucket, sweeper)
}

#[tokio::test(start_paused = true)]
async fn a_leader_whose_lease_runs_out_between_two_steps_of_a_save_writes_and_deletes_nothing_more()
{
    let (bucket, sweeper) = leader_for_five_seconds();
    let key = save_key();
    let attempt = AttemptId::mint();
    put_save(&bucket, &key, &[attempt], attempt, NOW - 5000);
    sweeper.sweep_once().await;
    assert_eq!(
        bucket.writes(),
        vec![format!("put {}", tombstone_key(&key))]
    );
    assert!(bucket.contains(&tombstone_key(&key)));
    assert!(bucket.contains(&manifest_key(&key)));
    assert!(bucket.contains(&part_object(&key, attempt)));
}

#[tokio::test(start_paused = true)]
async fn a_leader_whose_lease_runs_out_between_two_parts_of_an_orphan_attempt_deletes_no_more_part()
{
    let (bucket, sweeper) = leader_for_five_seconds();
    let key = save_key();
    let first = part_object(&key, AttemptId::mint());
    let second = first.replace("part-0", "part-1");
    bucket.insert(&first, b"part", NOW - 5000);
    bucket.insert(&second, b"part", NOW - 5000);
    sweeper.sweep_once().await;
    assert_eq!(bucket.writes(), vec![format!("delete {first}")]);
    assert!(bucket.contains(&second));
    assert!(!metric_of(
        &sweeper,
        "br_sandbox_keeper_orphans_deleted_total 1"
    ));
}

fn metric_of(sweeper: &Sweeper, line: &str) -> bool {
    sweeper
        .metrics
        .render(true)
        .lines()
        .any(|text| text == line)
}

#[tokio::test(start_paused = true)]
async fn a_restarted_sweeper_finishes_the_work_from_the_store_alone_and_keeps_a_live_save() {
    let harness = harness(true, 100);
    let (expired, live) = (save_key(), save_key());
    let (old_attempt, new_attempt) = (AttemptId::mint(), AttemptId::mint());
    put_save(
        &harness.bucket,
        &expired,
        &[old_attempt],
        old_attempt,
        NOW - 5000,
    );
    put_save(
        &harness.bucket,
        &live,
        &[new_attempt],
        new_attempt,
        NOW - 10,
    );
    harness.bucket.fail_delete(&manifest_key(&expired));
    harness.sweeper.sweep_once().await;
    assert!(harness.bucket.contains(&tombstone_key(&expired)));
    let kept = Arc::new(FakeBucket::default());
    for name in harness.bucket.keys() {
        kept.insert(&name, &harness.bucket.body(&name).unwrap(), NOW - 5000);
    }
    let clock: Arc<dyn Clock> = Arc::new(TokioClock::starting_at(at(NOW)));
    let leadership = Leadership::new(clock.clone());
    leadership.renewed(at(NOW + 1_000_000));
    let restarted = Sweeper {
        bucket: Arc::new(GuardedBucket::new(kept.clone())),
        clock,
        leadership,
        metrics: Arc::new(Metrics::new()),
        ..harness_sweeper(&harness)
    };
    restarted.sweep_once().await;
    assert!(!kept.contains(&manifest_key(&expired)));
    assert!(!kept.contains(&part_object(&expired, old_attempt)));
    assert!(kept.contains(&manifest_key(&live)));
    assert!(kept.contains(&part_object(&live, new_attempt)));
    assert!(
        restarted
            .metrics
            .render(true)
            .lines()
            .any(|text| text == "br_sandbox_keeper_sweep_errors_total 0")
    );
}
