use std::collections::BTreeMap;
use std::time::{Duration, UNIX_EPOCH};

use br_runner_sandbox::{LivePolicy, LiveVerdict, PodKeys, PodMarks, Timestamp, judge_live_pod};

use crate::common::turn;

fn at(seconds: u64) -> Timestamp {
    Timestamp::from_system_time(UNIX_EPOCH + Duration::from_secs(seconds))
}

const NOW: u64 = 10_000;

fn policy() -> LivePolicy {
    LivePolicy {
        save_ttl: Duration::from_secs(3600),
        idle_ttl: Duration::from_secs(120),
        idle_keep_min_state_bytes: 10_000_000,
    }
}

fn marks(parent: &str) -> PodMarks {
    PodMarks {
        clean: Some(true),
        state_turn: Some(parent.to_owned()),
        saved_at: Some(at(NOW - 100)),
        idle_since: Some(at(NOW - 10)),
        claimed_at: None,
        state_bytes: Some(20_000_000),
        seed_hash: Some("hash".to_owned()),
    }
}

fn verdict(marks: &PodMarks, parent: &str, hash: &str) -> LiveVerdict {
    let parent = br_agentic_harness_contract::TurnId::parse(parent).expect("turn");
    judge_live_pod(marks, parent, hash, at(NOW), &policy())
}

#[test]
fn a_clean_pod_of_the_parent_turn_with_a_fresh_save_and_idle_time_is_reusable() {
    let parent = turn().to_string();
    assert_eq!(
        verdict(&marks(&parent), &parent, "hash"),
        LiveVerdict::Reusable
    );
}

#[test]
fn a_pod_with_clean_false_or_no_clean_mark_is_never_a_candidate() {
    let parent = turn().to_string();
    for clean in [Some(false), None] {
        let mut pod = marks(&parent);
        pod.clean = clean;
        assert_eq!(verdict(&pod, &parent, "hash"), LiveVerdict::NotCandidate);
    }
}

#[test]
fn a_pod_whose_state_turn_is_not_the_parent_is_not_a_candidate() {
    let parent = turn().to_string();
    let pod = marks(&turn().to_string());
    assert_eq!(verdict(&pod, &parent, "hash"), LiveVerdict::NotCandidate);
    let mut none = marks(&parent);
    none.state_turn = None;
    assert_eq!(verdict(&none, &parent, "hash"), LiveVerdict::NotCandidate);
}

#[test]
fn a_candidate_whose_save_time_plus_the_ttl_has_passed_is_expired_with_that_time() {
    let parent = turn().to_string();
    let mut pod = marks(&parent);
    pod.saved_at = Some(at(NOW - 3600));
    assert_eq!(
        verdict(&pod, &parent, "hash"),
        LiveVerdict::Expired {
            expired_at: at(NOW)
        }
    );
    pod.saved_at = Some(at(NOW - 5000));
    assert_eq!(
        verdict(&pod, &parent, "hash"),
        LiveVerdict::Expired {
            expired_at: at(NOW - 1400)
        }
    );
}

#[test]
fn the_expiry_is_judged_before_the_other_conditions() {
    let parent = turn().to_string();
    let mut pod = marks(&parent);
    pod.saved_at = Some(at(NOW - 4000));
    pod.seed_hash = Some("other".to_owned());
    pod.idle_since = None;
    assert!(matches!(
        verdict(&pod, &parent, "hash"),
        LiveVerdict::Expired { .. }
    ));
}

#[test]
fn a_candidate_with_no_idle_time_or_an_idle_time_that_has_passed_is_not_reusable() {
    let parent = turn().to_string();
    let mut pod = marks(&parent);
    pod.idle_since = None;
    assert_eq!(verdict(&pod, &parent, "hash"), LiveVerdict::NotReusable);
    pod.idle_since = Some(at(NOW - 120));
    assert_eq!(verdict(&pod, &parent, "hash"), LiveVerdict::NotReusable);
    pod.idle_since = Some(at(NOW - 119));
    assert_eq!(verdict(&pod, &parent, "hash"), LiveVerdict::Reusable);
}

#[test]
fn a_candidate_with_a_state_below_the_threshold_is_not_reusable() {
    let parent = turn().to_string();
    let mut pod = marks(&parent);
    pod.state_bytes = Some(9_999_999);
    assert_eq!(verdict(&pod, &parent, "hash"), LiveVerdict::NotReusable);
    pod.state_bytes = Some(10_000_000);
    assert_eq!(verdict(&pod, &parent, "hash"), LiveVerdict::Reusable);
    pod.state_bytes = None;
    assert_eq!(verdict(&pod, &parent, "hash"), LiveVerdict::NotReusable);
}

#[test]
fn a_candidate_whose_seed_hash_is_absent_or_not_the_hash_of_the_request_is_not_reusable() {
    let parent = turn().to_string();
    let mut pod = marks(&parent);
    assert_eq!(verdict(&pod, &parent, "another"), LiveVerdict::NotReusable);
    pod.seed_hash = None;
    assert_eq!(verdict(&pod, &parent, "hash"), LiveVerdict::NotReusable);
}

#[test]
fn a_candidate_with_no_save_time_is_not_reusable_and_not_deleted() {
    let parent = turn().to_string();
    let mut pod = marks(&parent);
    pod.saved_at = None;
    assert_eq!(verdict(&pod, &parent, "hash"), LiveVerdict::NotReusable);
}

#[test]
fn the_marks_are_read_from_the_annotations_with_the_keys_of_the_prefix() {
    let keys = PodKeys::default();
    let turn = turn().to_string();
    let annotations = BTreeMap::from([
        (keys.clean(), "true".to_owned()),
        (keys.state_turn(), turn.clone()),
        (keys.saved_at(), "2026-11-26T10:00:00.000Z".to_owned()),
        (keys.idle_since(), "2026-11-26T10:01:00.000Z".to_owned()),
        (keys.claimed_at(), "2026-11-26T09:00:00.000Z".to_owned()),
        (keys.state_bytes(), "12345".to_owned()),
        (keys.seed_hash(), "abc".to_owned()),
    ]);
    let read = PodMarks::read(&annotations, &keys);
    assert_eq!(read.clean, Some(true));
    assert_eq!(read.state_turn.as_deref(), Some(turn.as_str()));
    assert_eq!(
        read.saved_at.map(|time| time.to_string()).as_deref(),
        Some("2026-11-26T10:00:00.000Z")
    );
    assert_eq!(
        read.idle_since.map(|time| time.to_string()).as_deref(),
        Some("2026-11-26T10:01:00.000Z")
    );
    assert_eq!(
        read.claimed_at.map(|time| time.to_string()).as_deref(),
        Some("2026-11-26T09:00:00.000Z")
    );
    assert_eq!(read.state_bytes, Some(12345));
    assert_eq!(read.seed_hash.as_deref(), Some("abc"));
}

#[test]
fn an_annotation_that_is_not_valid_is_read_as_absent() {
    let keys = PodKeys::default();
    let annotations = BTreeMap::from([
        (keys.clean(), "yes".to_owned()),
        (keys.saved_at(), "yesterday".to_owned()),
        (keys.state_bytes(), "many".to_owned()),
    ]);
    assert_eq!(PodMarks::read(&annotations, &keys), PodMarks::default());
}

#[test]
fn the_policy_comes_from_the_configuration() {
    let policy = LivePolicy::of(&crate::common::config());
    assert_eq!(policy.save_ttl, Duration::from_secs(3600));
    assert_eq!(policy.idle_ttl, Duration::from_secs(120));
    assert_eq!(policy.idle_keep_min_state_bytes, 10_000_000);
}
