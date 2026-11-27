use super::*;

#[tokio::test(start_paused = true)]
async fn a_tombstone_written_late_goes_when_the_ttl_has_passed_since_its_expiry_and_not_since_its_write()
 {
    let harness = harness(true, 100);
    let key = save_key_born(10);
    let late = Tombstone::new(key.turn(), at(10), at(20));
    harness
        .bucket
        .insert(&tombstone_key(&key), &late.to_vec().unwrap(), NOW - 5);
    harness.sweeper.sweep_once().await;
    assert!(!harness.bucket.contains(&tombstone_key(&key)));
}

#[tokio::test(start_paused = true)]
async fn a_record_stored_late_goes_when_the_ttl_has_passed_since_its_end_and_not_since_its_store_time()
 {
    let harness = harness(true, 100);
    let key = save_key_born(10);
    let record = Record::started(AttemptId::mint(), at(100)).done(at(200));
    harness
        .bucket
        .insert(&record_key(&key), &record.to_vec().unwrap(), NOW - 1);
    harness.sweeper.sweep_once().await;
    assert!(!harness.bucket.contains(&record_key(&key)));
}

#[tokio::test(start_paused = true)]
async fn an_object_with_an_old_store_time_is_not_read_while_its_turn_is_too_young_to_be_old() {
    let harness = harness(true, 100);
    let (record_key_of, tombstone_key_of) = (save_key_born(NOW - 10), save_key_born(NOW - 100));
    let record = Record::started(AttemptId::mint(), at(NOW - 10));
    let tombstone = Tombstone::new(tombstone_key_of.turn(), at(NOW - 100), at(NOW - 50));
    harness
        .bucket
        .insert(&record_key(&record_key_of), &record.to_vec().unwrap(), 0);
    harness.bucket.insert(
        &tombstone_key(&tombstone_key_of),
        &tombstone.to_vec().unwrap(),
        0,
    );
    harness.sweeper.sweep_once().await;
    assert!(harness.bucket.contains(&record_key(&record_key_of)));
    assert!(harness.bucket.contains(&tombstone_key(&tombstone_key_of)));
    assert!(
        !harness
            .bucket
            .log()
            .iter()
            .any(|line| line.starts_with("get "))
    );
}
