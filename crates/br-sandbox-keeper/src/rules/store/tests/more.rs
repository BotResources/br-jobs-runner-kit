use super::*;

#[test]
fn an_expired_save_is_removed_tombstone_then_manifest_then_every_part_and_the_record_stays() {
    let key = save_key();
    let (named, rival) = (AttemptId::mint(), AttemptId::mint());
    let made = manifest(&key, named, at(10_000));
    let turn = listing(&key, Some(10_000), &[(named, 10_000), (rival, 10_001)]);
    let steps = plan_turn(
        &turn,
        &ManifestRead::Parsed(Box::new(made.clone())),
        at(11_061),
        &rules(),
    );
    let tombstone = tombstone_for(&made, rules().save_ttl).to_vec().unwrap();
    assert_eq!(
        steps[0],
        StoreStep::PutTombstone {
            key: br_runner_sandbox::tombstone_key(&key),
            body: tombstone,
        }
    );
    assert_eq!(
        steps[1],
        StoreStep::DeleteManifest {
            key: br_runner_sandbox::manifest_key(&key),
        }
    );
    let mut parts: Vec<_> = steps[2..]
        .iter()
        .map(|step| match step {
            StoreStep::DeleteExpiredPart { key } => key.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    parts.sort();
    let mut expected = vec![part_key(&key, named), part_key(&key, rival)];
    expected.sort();
    assert_eq!(parts, expected);
    assert!(steps.iter().all(|step| match step {
        StoreStep::PutTombstone { key, .. }
        | StoreStep::DeleteManifest { key }
        | StoreStep::DeleteExpiredPart { key } => !key.starts_with("records/"),
        StoreStep::DeleteOrphanAttempt { .. } => false,
    }));
}

#[test]
fn the_parts_that_a_manifest_names_are_never_an_orphan() {
    let key = save_key();
    let named = AttemptId::mint();
    let made = manifest(&key, named, at(10_000));
    let turn = listing(&key, Some(10_000), &[(named, 10_000)]);
    assert!(
        plan_turn(
            &turn,
            &ManifestRead::Parsed(Box::new(made)),
            at(10_500),
            &long_lived()
        )
        .is_empty()
    );
}

#[test]
fn a_turn_with_no_manifest_has_orphan_attempts_once_old_and_a_live_upload_is_never_touched() {
    let key = save_key();
    let (old, live) = (AttemptId::mint(), AttemptId::mint());
    let turn = listing(&key, None, &[(old, 1000), (live, 6000)]);
    let steps = plan_turn(&turn, &ManifestRead::Absent, at(5000), &rules());
    assert_eq!(
        steps,
        vec![StoreStep::DeleteOrphanAttempt {
            keys: vec![part_key(&key, old)]
        }]
    );
    let none = plan_turn(&turn, &ManifestRead::Absent, at(4600), &rules());
    assert!(none.is_empty());
}

#[test]
fn the_age_of_an_attempt_counts_from_its_newest_object() {
    let key = save_key();
    let attempt = AttemptId::mint();
    let dir = part_key(&key, attempt)
        .trim_end_matches("part-0.tar.zst")
        .to_owned();
    let mut grouper = TurnGrouper::default();
    grouper.push(info(&format!("{dir}part-0.tar.zst"), 100));
    grouper.push(info(&format!("{dir}part-1.tar.zst"), 9000));
    let turn = grouper.finish().unwrap();
    assert!(plan_turn(&turn, &ManifestRead::Absent, at(9000 + 3600), &rules()).is_empty());
    assert_eq!(
        plan_turn(&turn, &ManifestRead::Absent, at(9000 + 3601), &rules()).len(),
        1
    );
}

#[test]
fn a_record_is_old_when_the_ttl_has_passed_since_its_end_or_its_start() {
    let key = save_key();
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
    assert!(record_key(&key).starts_with("records/"));
}

#[test]
fn a_tombstone_is_old_when_the_ttl_has_passed_since_its_expiry_without_the_margin() {
    let key = save_key();
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
fn an_object_is_read_for_age_only_when_its_own_time_is_older_than_the_ttl() {
    let ttl = rules().tombstone_ttl.as_secs();
    assert!(!needs_read(
        &info("records/x", 100),
        at(100 + ttl - 1),
        &rules()
    ));
    assert!(needs_read(&info("records/x", 100), at(100 + ttl), &rules()));
}
