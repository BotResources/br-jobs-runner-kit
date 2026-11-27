use super::*;

fn crash_after_the_manifest(
    harness: &Harness,
    key: &SaveKey,
    attempt: AttemptId,
    made: u64,
) -> Sweeper {
    put_save(&harness.bucket, key, &[attempt], attempt, made);
    harness.bucket.fail_delete(&part_object(key, attempt));
    Sweeper {
        bucket: Arc::new(GuardedBucket::new(harness.bucket.clone())),
        ..harness_sweeper(harness)
    }
}

fn restarted_on_a_healthy_store(
    harness: &Harness,
    key: &SaveKey,
    attempt: AttemptId,
    made: u64,
) -> (Arc<FakeBucket>, Sweeper) {
    let healthy = Arc::new(FakeBucket::default());
    healthy.insert(&part_object(key, attempt), b"part", made);
    let sweeper = Sweeper {
        bucket: Arc::new(GuardedBucket::new(healthy.clone())),
        ..harness_sweeper(harness)
    };
    (healthy, sweeper)
}

#[tokio::test(start_paused = true)]
async fn a_tombstone_that_cannot_be_serialized_is_logged_counted_and_leaves_the_save_whole() {
    let (logs, _guard) = crate::testing::logs::capture();
    let harness = harness(true, 100);
    let key = save_key();
    let attempt = AttemptId::mint();
    put_save(&harness.bucket, &key, &[attempt], attempt, NOW - 5000);
    let step = StoreStep::TombstoneNotSerialized {
        key: tombstone_key(&key),
    };
    assert!(matches!(
        harness.sweeper.run_step(step).await,
        Err(Stop::Failed)
    ));
    assert!(metric(&harness, "br_sandbox_keeper_sweep_errors_total 1"));
    assert!(logs.text().contains(&tombstone_key(&key)));
    assert!(harness.bucket.writes().is_empty());
    assert!(harness.bucket.contains(&manifest_key(&key)));
}

#[tokio::test(start_paused = true)]
async fn a_part_left_by_a_crash_after_the_manifest_is_removed_at_the_next_sweep_when_it_is_older_than_the_attempt_ttl()
 {
    let harness = harness(true, 100);
    let (key, attempt) = (save_key(), AttemptId::mint());
    let made = NOW - 5000;
    let crashed = crash_after_the_manifest(&harness, &key, attempt, made);
    crashed.sweep_once().await;
    assert!(!harness.bucket.contains(&manifest_key(&key)));
    assert!(harness.bucket.contains(&tombstone_key(&key)));
    assert!(harness.bucket.contains(&part_object(&key, attempt)));

    let (healthy, next) = restarted_on_a_healthy_store(&harness, &key, attempt, made);
    next.sweep_once().await;
    assert!(!healthy.contains(&part_object(&key, attempt)));
}

#[tokio::test(start_paused = true)]
async fn a_part_left_by_a_crash_after_the_manifest_stays_until_it_is_older_than_the_attempt_ttl() {
    let harness = harness(true, 100);
    let (key, attempt) = (save_key(), AttemptId::mint());
    let made = NOW - 2000;
    let crashed = crash_after_the_manifest(&harness, &key, attempt, made);
    crashed.sweep_once().await;
    assert!(!harness.bucket.contains(&manifest_key(&key)));
    assert!(harness.bucket.contains(&part_object(&key, attempt)));

    let (healthy, next) = restarted_on_a_healthy_store(&harness, &key, attempt, made);
    next.sweep_once().await;
    assert!(healthy.contains(&part_object(&key, attempt)));
    tokio::time::advance(Duration::from_secs(1600)).await;
    next.sweep_once().await;
    assert!(healthy.contains(&part_object(&key, attempt)));
    tokio::time::advance(Duration::from_secs(1)).await;
    next.sweep_once().await;
    assert!(!healthy.contains(&part_object(&key, attempt)));
}
