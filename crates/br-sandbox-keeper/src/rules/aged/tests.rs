use std::time::Duration;

use br_runner_sandbox::{AttemptId, Record};

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
