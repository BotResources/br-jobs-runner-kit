use std::collections::BTreeMap;

use br_runner_sandbox::{ConditionView, ImageError, LabelSelector, PodView, ReplicaSetView};
use serde_json::json;

use crate::fakes::pod;

fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn replica_set(conditions: Vec<ConditionView>) -> ReplicaSetView {
    ReplicaSetView {
        name: "warm-main".to_owned(),
        conditions,
        template: json!({}),
    }
}

fn condition(kind: &str, status: &str) -> ConditionView {
    ConditionView {
        kind: kind.to_owned(),
        status: status.to_owned(),
        reason: Some("ImagePullFailing".to_owned()),
        message: Some("back-off pulling image".to_owned()),
    }
}

#[test]
fn a_label_selector_is_the_sorted_list_of_its_pairs() {
    let selector = LabelSelector::new()
        .with("br.sandbox/thread", "t1")
        .with("br.sandbox/pool", "claimed");
    assert_eq!(
        selector.to_string(),
        "br.sandbox/pool=claimed,br.sandbox/thread=t1"
    );
    assert_eq!(LabelSelector::new().to_string(), "");
}

#[test]
fn a_label_selector_matches_only_the_pods_that_hold_every_pair() {
    let selector = LabelSelector::new()
        .with("pool", "warm")
        .with("pool-id", "main");
    assert!(selector.matches(&labels(&[
        ("pool", "warm"),
        ("pool-id", "main"),
        ("x", "y")
    ])));
    assert!(!selector.matches(&labels(&[("pool", "claimed"), ("pool-id", "main")])));
    assert!(!selector.matches(&labels(&[("pool", "warm")])));
    assert!(LabelSelector::new().matches(&labels(&[])));
}

#[test]
fn a_pod_is_ready_only_when_running_with_the_ready_condition_and_no_deletion_time() {
    let ready = pod("p");
    assert!(ready.is_ready());
    let cases: Vec<(&str, PodView)> = vec![
        (
            "pending",
            PodView {
                phase: Some("Pending".to_owned()),
                ..pod("p")
            },
        ),
        (
            "no phase",
            PodView {
                phase: None,
                ..pod("p")
            },
        ),
        (
            "not ready",
            PodView {
                ready_condition: false,
                ..pod("p")
            },
        ),
        (
            "deleting",
            PodView {
                deleting: true,
                ..pod("p")
            },
        ),
    ];
    for (name, view) in cases {
        assert!(!view.is_ready(), "{name}");
    }
}

#[test]
fn the_pool_condition_is_an_image_failure_only_when_its_status_is_true() {
    let failing = replica_set(vec![condition("WarmPodsFailing", "True")]);
    assert_eq!(
        failing.warm_pods_failing(),
        Some(ImageError::failing(
            "ImagePullFailing",
            "back-off pulling image"
        ))
    );
    assert_eq!(
        replica_set(vec![condition("WarmPodsFailing", "False")]).warm_pods_failing(),
        None
    );
    assert_eq!(
        replica_set(vec![condition("WarmPodsFailing", "Unknown")]).warm_pods_failing(),
        None
    );
}

#[test]
fn an_absent_pool_condition_and_a_condition_of_another_type_are_read_as_not_true() {
    assert_eq!(replica_set(Vec::new()).warm_pods_failing(), None);
    assert_eq!(
        replica_set(vec![condition("ReplicaFailure", "True")]).warm_pods_failing(),
        None
    );
}

#[test]
fn the_image_failure_keeps_a_missing_reason_and_message_as_empty_text() {
    let bare = ConditionView {
        kind: "WarmPodsFailing".to_owned(),
        status: "True".to_owned(),
        reason: None,
        message: None,
    };
    assert_eq!(
        replica_set(vec![bare]).warm_pods_failing(),
        Some(ImageError::failing("", ""))
    );
}
