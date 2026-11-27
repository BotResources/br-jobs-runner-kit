use super::*;

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
