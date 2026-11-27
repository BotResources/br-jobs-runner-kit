use std::time::Duration;

use br_runner_sandbox::{AttemptId, Record, record_key, tombstone_key};

use super::*;
use crate::rules::tombstone_for;
use crate::testing::saves::{manifest, save_key_born};
use crate::time::at;

fn rules() -> SweepRules {
    SweepRules::new(
        Duration::from_secs(1000),
        Duration::from_secs(60),
        Duration::from_secs(3600),
        Duration::from_secs(10_000),
    )
}

fn info(key: &str, modified: u64) -> ObjectInfo {
    ObjectInfo::new(key.to_owned(), at(modified))
}

#[test]
fn a_record_is_old_when_the_ttl_has_passed_since_its_end_or_its_start() {
    let started = Record::started(AttemptId::mint(), at(1000));
    let done = started.done(at(5000));
    let ttl = rules().tombstone_ttl.as_secs();
    assert_eq!(
        record_is_old(&started.to_vec().unwrap(), at(1000 + ttl - 1), &rules()),
        Ok(false)
    );
    assert_eq!(
        record_is_old(&started.to_vec().unwrap(), at(1000 + ttl), &rules()),
        Ok(true)
    );
    assert_eq!(
        record_is_old(&done.to_vec().unwrap(), at(5000 + ttl - 1), &rules()),
        Ok(false)
    );
    assert_eq!(
        record_is_old(&done.to_vec().unwrap(), at(5000 + ttl), &rules()),
        Ok(true)
    );
    assert!(record_is_old(b"{", at(0), &rules()).is_err());
}

#[test]
fn a_tombstone_is_old_when_the_ttl_has_passed_since_its_expiry_without_the_margin() {
    let key = save_key_born(100);
    let made = manifest(&key, AttemptId::mint(), at(10_000));
    let bytes = tombstone_for(&made, Duration::from_secs(1000))
        .to_vec()
        .unwrap();
    let ttl = rules().tombstone_ttl.as_secs();
    assert_eq!(
        tombstone_is_old(&bytes, at(11_000 + ttl - 1), &rules()),
        Ok(false)
    );
    assert_eq!(
        tombstone_is_old(&bytes, at(11_000 + ttl), &rules()),
        Ok(true)
    );
    assert!(tombstone_is_old(b"x", at(0), &rules()).is_err());
}

#[test]
fn a_record_is_read_from_the_birth_of_its_turn_plus_the_ttl_less_the_skew_margin() {
    let key = save_key_born(5000);
    let record = info(&record_key(&key), 5_000_000);
    let read_from = 5000 + 10_000 - 60;
    assert!(!record_needs_read(&record, at(read_from - 1), &rules()));
    assert!(record_needs_read(&record, at(read_from), &rules()));
}

#[test]
fn a_tombstone_is_read_from_the_birth_of_its_turn_plus_the_save_ttl_and_the_ttl_less_the_margin() {
    let key = save_key_born(5000);
    let tombstone = info(&tombstone_key(&key), 5_000_000);
    let read_from = 5000 + 1000 + 10_000 - 60;
    assert!(!tombstone_needs_read(
        &tombstone,
        at(read_from - 1),
        &rules()
    ));
    assert!(tombstone_needs_read(&tombstone, at(read_from), &rules()));
}

#[test]
fn the_time_that_the_store_gives_an_object_never_keeps_it_from_being_read() {
    let key = save_key_born(100);
    let now = at(100 + 1000 + 10_000);
    for late in [0, 100 + 1000 + 10_000 - 1, 100 + 1000 + 10_000] {
        assert!(tombstone_needs_read(
            &info(&tombstone_key(&key), late),
            now,
            &rules()
        ));
        assert!(record_needs_read(
            &info(&record_key(&key), late),
            now,
            &rules()
        ));
    }
}

#[test]
fn an_object_whose_key_does_not_give_the_birth_of_a_turn_is_always_read() {
    let now = at(1);
    for key in [
        "records/acme/t/a/not-a-turn.json",
        "records/acme/t/a/0190c0de-0000-4000-8000-000000000000.json",
        "records/acme/t/a/0190c0de-0000-7000-8000-000000000000.txt",
        "records/acme",
        "",
    ] {
        assert!(record_needs_read(&info(key, 1), now, &rules()), "{key}");
        assert!(tombstone_needs_read(&info(key, 1), now, &rules()), "{key}");
    }
}
