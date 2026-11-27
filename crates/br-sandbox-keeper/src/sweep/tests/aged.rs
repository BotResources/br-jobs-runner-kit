use super::*;

fn put_record(harness: &Harness, key: &SaveKey, record: &Record) {
    harness
        .bucket
        .insert(&record_key(key), &record.to_vec().unwrap(), NOW - 1);
}

fn put_tombstone(harness: &Harness, key: &SaveKey, expired_at: u64) {
    let tombstone = Tombstone::new(key.turn(), at(expired_at - 1000), at(expired_at));
    harness
        .bucket
        .insert(&tombstone_key(key), &tombstone.to_vec().unwrap(), NOW - 1);
}

#[tokio::test(start_paused = true)]
async fn a_record_ended_longer_ago_than_the_ttl_goes_and_one_not_yet_old_stays() {
    let harness = harness(true, 100);
    let (old, young) = (save_key(), save_key());
    let ttl = rules().tombstone_ttl.as_secs();
    put_record(
        &harness,
        &old,
        &Record::started(AttemptId::mint(), at(10)).done(at(NOW - ttl)),
    );
    put_record(
        &harness,
        &young,
        &Record::started(AttemptId::mint(), at(10)).done(at(NOW - ttl + 1)),
    );
    harness.sweeper.sweep_once().await;
    assert!(!harness.bucket.contains(&record_key(&old)));
    assert!(harness.bucket.contains(&record_key(&young)));
}

#[tokio::test(start_paused = true)]
async fn a_record_with_no_end_time_is_judged_from_its_start_time() {
    let harness = harness(true, 100);
    let (old, young) = (save_key(), save_key());
    let ttl = rules().tombstone_ttl.as_secs();
    put_record(
        &harness,
        &old,
        &Record::started(AttemptId::mint(), at(NOW - ttl)),
    );
    put_record(
        &harness,
        &young,
        &Record::started(AttemptId::mint(), at(NOW - ttl + 1)),
    );
    harness.sweeper.sweep_once().await;
    assert!(!harness.bucket.contains(&record_key(&old)));
    assert!(harness.bucket.contains(&record_key(&young)));
}

#[tokio::test(start_paused = true)]
async fn a_record_stored_late_goes_when_the_ttl_has_passed_since_its_end_and_not_since_its_store_time()
 {
    let harness = harness(true, 100);
    let key = save_key();
    let record = Record::started(AttemptId::mint(), at(100)).done(at(200));
    harness
        .bucket
        .insert(&record_key(&key), &record.to_vec().unwrap(), NOW - 1);
    harness.sweeper.sweep_once().await;
    assert!(!harness.bucket.contains(&record_key(&key)));
}

#[tokio::test(start_paused = true)]
async fn a_tombstone_is_old_when_the_ttl_has_passed_since_its_expiry_and_not_before() {
    let harness = harness(true, 100);
    let (old, young) = (save_key(), save_key());
    let ttl = rules().tombstone_ttl.as_secs();
    put_tombstone(&harness, &old, NOW - ttl);
    put_tombstone(&harness, &young, NOW - ttl + 1);
    harness.sweeper.sweep_once().await;
    assert!(!harness.bucket.contains(&tombstone_key(&old)));
    assert!(harness.bucket.contains(&tombstone_key(&young)));
}

#[tokio::test(start_paused = true)]
async fn a_tombstone_written_late_goes_when_the_ttl_has_passed_since_its_expiry_and_not_since_its_write()
 {
    let harness = harness(true, 100);
    let key = save_key();
    let late = Tombstone::new(key.turn(), at(10), at(20));
    harness
        .bucket
        .insert(&tombstone_key(&key), &late.to_vec().unwrap(), NOW - 5);
    harness.sweeper.sweep_once().await;
    assert!(!harness.bucket.contains(&tombstone_key(&key)));
}

#[tokio::test(start_paused = true)]
async fn an_unreadable_record_or_tombstone_is_kept_logged_and_counted() {
    let (logs, _guard) = crate::testing::logs::capture();
    let harness = harness(true, 100);
    let (record, tombstone) = (save_key(), save_key());
    harness
        .bucket
        .insert(&record_key(&record), br#"{"SECRET-CONTENT-2":"x"}"#, 1);
    harness.bucket.insert(&tombstone_key(&tombstone), b"{", 1);
    harness.sweeper.sweep_once().await;
    assert!(harness.bucket.contains(&record_key(&record)));
    assert!(harness.bucket.contains(&tombstone_key(&tombstone)));
    assert!(harness.bucket.writes().is_empty());
    assert!(metric(&harness, "br_sandbox_keeper_sweep_errors_total 2"));
    let text = logs.text();
    assert!(text.contains(&record_key(&record)));
    assert!(text.contains(&tombstone_key(&tombstone)));
    assert!(!text.contains("SECRET-CONTENT-2"));
}

#[tokio::test(start_paused = true)]
async fn an_object_that_the_store_fails_to_give_is_kept_and_counted() {
    let harness = harness(true, 100);
    let (record, tombstone) = (save_key(), save_key());
    put_record(
        &harness,
        &record,
        &Record::started(AttemptId::mint(), at(10)).done(at(20)),
    );
    put_tombstone(&harness, &tombstone, 2000);
    harness.bucket.fail_get(&record_key(&record));
    harness.bucket.fail_get(&tombstone_key(&tombstone));
    harness.sweeper.sweep_once().await;
    assert!(harness.bucket.contains(&record_key(&record)));
    assert!(harness.bucket.contains(&tombstone_key(&tombstone)));
    assert!(metric(&harness, "br_sandbox_keeper_sweep_errors_total 2"));
}

#[tokio::test(start_paused = true)]
async fn every_listed_record_and_tombstone_is_read_once_in_a_sweep_whatever_its_age_and_the_page_size()
 {
    let harness = harness(true, 2);
    let ttl = rules().tombstone_ttl.as_secs();
    let mut reads = Vec::new();
    for ended in [NOW - 10, NOW - ttl, NOW - ttl - 5000, NOW - 1, 20] {
        let key = save_key();
        put_record(
            &harness,
            &key,
            &Record::started(AttemptId::mint(), at(10)).done(at(ended)),
        );
        put_tombstone(&harness, &key, ended.max(1000));
        reads.push(format!("get {}", record_key(&key)));
        reads.push(format!("get {}", tombstone_key(&key)));
    }
    harness.sweeper.sweep_once().await;
    let log = harness.bucket.log();
    for read in &reads {
        assert_eq!(log.iter().filter(|line| *line == read).count(), 1, "{read}");
    }
    let objects_read = log
        .iter()
        .filter(|line| line.starts_with("get records/") || line.starts_with("get tombstones/"))
        .count();
    assert_eq!(objects_read, 10);
}
