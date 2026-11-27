use super::*;
use crate::testing::saves::{manifest, save_key};
use crate::time::at;
use br_runner_sandbox::{AttemptId, Record, SaveKey, Tombstone, part_key, record_key};

mod more;

fn rules() -> SweepRules {
    SweepRules {
        save_ttl: Duration::from_secs(1000),
        skew_margin: Duration::from_secs(60),
        attempt_ttl: Duration::from_secs(3600),
        tombstone_ttl: Duration::from_secs(31_536_000),
    }
}

fn long_lived() -> SweepRules {
    SweepRules {
        save_ttl: Duration::from_secs(1_000_000),
        ..rules()
    }
}

fn info(key: &str, modified: u64) -> ObjectInfo {
    ObjectInfo {
        key: key.to_owned(),
        last_modified: at(modified),
    }
}

fn listing(key: &SaveKey, manifest_at: Option<u64>, attempts: &[(AttemptId, u64)]) -> TurnListing {
    let mut grouper = TurnGrouper::default();
    let mut objects = Vec::new();
    for (attempt, modified) in attempts {
        objects.push(info(&part_key(key, *attempt), *modified));
    }
    if let Some(modified) = manifest_at {
        objects.push(info(&br_runner_sandbox::manifest_key(key), modified));
    }
    for object in objects {
        assert!(grouper.push(object).is_none());
    }
    grouper.finish().unwrap()
}

#[test]
fn a_save_is_expired_only_above_the_age_plus_the_margin() {
    let made = at(10_000);
    assert!(!save_expired(made, at(11_060), &rules()));
    assert!(save_expired(made, at(11_061), &rules()));
    assert!(!save_expired(made, at(10_000), &rules()));
}

#[test]
fn a_zero_margin_expires_the_save_just_above_the_age() {
    let mut none = rules();
    none.skew_margin = Duration::ZERO;
    assert!(!save_expired(at(0), at(1000), &none));
    assert!(save_expired(at(0), at(1001), &none));
}

#[test]
fn the_tombstone_is_a_pure_function_of_the_manifest_and_the_age_without_the_margin() {
    let key = save_key();
    let attempt = AttemptId::mint();
    let manifest = manifest(&key, attempt, at(10_000));
    let tombstone = tombstone_for(&manifest, Duration::from_secs(1000));
    assert_eq!(tombstone.turn(), key.turn().to_string());
    assert_eq!(tombstone.made_at(), at(10_000));
    assert_eq!(tombstone.expired_at(), at(11_000));
    assert_eq!(tombstone.reason(), "expired");
    let again = tombstone_for(&manifest, Duration::from_secs(1000));
    assert_eq!(tombstone.to_vec().unwrap(), again.to_vec().unwrap());
    assert_eq!(
        Tombstone::parse(&tombstone.to_vec().unwrap()).unwrap(),
        tombstone
    );
}

#[test]
fn the_grouper_groups_the_objects_of_a_turn_and_keeps_the_newest_time_of_an_attempt() {
    let key = save_key();
    let (first, second) = (AttemptId::mint(), AttemptId::mint());
    let mut grouper = TurnGrouper::default();
    for object in [
        info(&part_key(&key, first), 100),
        info(&part_key(&key, second), 300),
        info(
            &format!(
                "{}/part-1.tar.zst",
                part_key(&key, second).trim_end_matches("/part-0.tar.zst")
            ),
            500,
        ),
        info(&br_runner_sandbox::manifest_key(&key), 600),
    ] {
        assert!(grouper.push(object).is_none());
    }
    let turn = grouper.finish().unwrap();
    assert_eq!(turn.attempts.len(), 2);
    assert_eq!(turn.attempts[&second.to_string()].newest, at(500));
    assert_eq!(turn.attempts[&second.to_string()].keys.len(), 2);
    assert_eq!(turn.manifest.unwrap().last_modified, at(600));
}

#[test]
fn the_grouper_emits_a_turn_when_the_next_turn_begins_and_ignores_other_keys() {
    let (one, two) = (save_key(), save_key());
    let mut grouper = TurnGrouper::default();
    assert!(
        grouper
            .push(info(&br_runner_sandbox::manifest_key(&one), 1))
            .is_none()
    );
    assert!(grouper.push(info("saves/readme.txt", 1)).is_none());
    assert!(grouper.push(info("saves/a/b/c/d/e/f/g/h", 1)).is_none());
    let done = grouper
        .push(info(&br_runner_sandbox::manifest_key(&two), 2))
        .unwrap();
    assert!(done.dir.ends_with(&one.turn().to_string()));
    assert!(
        grouper
            .finish()
            .unwrap()
            .dir
            .ends_with(&two.turn().to_string())
    );
    assert!(grouper.finish().is_none());
}

#[test]
fn a_manifest_is_read_when_it_may_be_expired_or_when_attempts_compete() {
    let key = save_key();
    let attempt = AttemptId::mint();
    let fresh = listing(&key, Some(10_000), &[(attempt, 10_000)]);
    assert!(!needs_manifest(&fresh, at(10_500), &rules()));
    assert!(needs_manifest(&fresh, at(11_061), &rules()));
    assert!(!needs_manifest(&fresh, at(10_000 + 3601), &long_lived()));
    let rival = AttemptId::mint();
    let two = listing(&key, Some(10_000), &[(attempt, 10_000), (rival, 10_000)]);
    assert!(!needs_manifest(&two, at(10_500), &rules()));
    assert!(needs_manifest(&two, at(10_000 + 3601), &long_lived()));
    assert!(!needs_manifest(
        &listing(&key, None, &[(attempt, 1)]),
        at(99_999),
        &rules()
    ));
}

#[test]
fn the_manifest_read_checks_that_the_manifest_belongs_to_its_key() {
    let key = save_key();
    let attempt = AttemptId::mint();
    let made = manifest(&key, attempt, at(10_000));
    let turn = listing(&key, Some(1), &[(attempt, 1)]);
    assert!(matches!(
        read_manifest(&turn, &made.to_vec().unwrap()),
        ManifestRead::Parsed(_)
    ));
    assert_eq!(read_manifest(&turn, b"{"), ManifestRead::Unreadable);
    let other = manifest(&save_key(), attempt, at(10_000));
    assert_eq!(
        read_manifest(&turn, &other.to_vec().unwrap()),
        ManifestRead::Unreadable
    );
    assert_eq!(
        read_manifest(&listing(&key, None, &[(attempt, 1)]), b"anything"),
        ManifestRead::Absent
    );
}

#[test]
fn a_save_inside_the_margin_is_left_whole() {
    let key = save_key();
    let attempt = AttemptId::mint();
    let made = manifest(&key, attempt, at(10_000));
    let turn = listing(&key, Some(10_000), &[(attempt, 10_000)]);
    assert!(
        plan_turn(
            &turn,
            &ManifestRead::Parsed(Box::new(made)),
            at(11_060),
            &rules()
        )
        .is_empty()
    );
}

#[test]
fn an_attempt_that_the_manifest_does_not_name_is_an_orphan_once_its_newest_object_is_old() {
    let key = save_key();
    let (named, loser) = (AttemptId::mint(), AttemptId::mint());
    let made = manifest(&key, named, at(10_000));
    let turn = listing(&key, Some(10_000), &[(named, 10_000), (loser, 10_001)]);
    let read = ManifestRead::Parsed(Box::new(made));
    assert!(plan_turn(&turn, &read, at(10_001 + 3600), &long_lived()).is_empty());
    let steps = plan_turn(&turn, &read, at(10_001 + 3601), &long_lived());
    assert_eq!(
        steps,
        vec![StoreStep::DeleteOrphanAttempt {
            keys: vec![part_key(&key, loser)]
        }]
    );
}

#[test]
fn a_manifest_that_cannot_be_read_gives_no_step_and_so_no_delete_for_age_or_orphan() {
    let key = save_key();
    let (a, b) = (AttemptId::mint(), AttemptId::mint());
    let turn = listing(&key, Some(1), &[(a, 1), (b, 1)]);
    assert!(plan_turn(&turn, &ManifestRead::Unreadable, at(99_999_999), &rules()).is_empty());
    assert!(plan_turn(&turn, &ManifestRead::NotNeeded, at(99_999_999), &rules()).is_empty());
}
