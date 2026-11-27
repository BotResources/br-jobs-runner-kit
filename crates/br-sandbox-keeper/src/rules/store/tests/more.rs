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
