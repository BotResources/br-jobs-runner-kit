use super::*;
use crate::rules::condition_write;

fn owned(refusal: Option<&str>, ready: u32) -> Observation {
    Observation::new(
        Counts::new(4, 5, ready, 3),
        ReplicaSetView::Owned {
            refusal: refusal.map(str::to_owned),
        },
        judgement(false),
        at(500),
    )
}

fn ready_of(observation: Option<&Observation>, min: Option<u32>) -> (String, String, String) {
    let status = compute_status(&StatusInput::new(
        Some(7),
        min,
        &Acceptance::Valid,
        observation,
        None,
        at(600),
    ));
    let ready = condition(&status, "Ready");
    (
        ready.status.clone(),
        ready.reason.clone(),
        ready.message.clone(),
    )
}

#[test]
fn a_valid_resource_with_a_synced_view_is_accepted_and_ready_with_the_counts() {
    let observation = observed(2, 500);
    let status = status_of(&Acceptance::Valid, Some(&observation), None, at(600));
    assert_eq!(condition(&status, "Accepted").status, "True");
    assert_eq!(condition(&status, "Accepted").reason, "Valid");
    assert_eq!(condition(&status, "Ready").status, "True");
    assert_eq!(status.observed_generation, Some(7));
    assert_eq!(status.desired_replicas, Some(4));
    assert_eq!(status.replicas, Some(5));
    assert_eq!(status.ready_replicas, Some(2));
    assert_eq!(status.updated_replicas, Some(3));
    assert_eq!(
        status
            .observed_at
            .as_ref()
            .map(crate::convert::timestamp_of),
        Some(at(500))
    );
}

#[test]
fn ready_needs_the_minimum_of_ready_warm_pods() {
    let (status, _, _) = ready_of(Some(&owned(None, 2)), Some(2));
    assert_eq!(status, "True");
    let (status, reason, message) = ready_of(Some(&owned(None, 1)), Some(2));
    assert_eq!(
        (status.as_str(), reason.as_str()),
        ("False", "WarmBelowMinimum")
    );
    assert!(message.contains("1") && message.contains("2"), "{message}");
}

#[test]
fn a_minimum_of_zero_is_ready_with_no_warm_pod() {
    let (status, _, _) = ready_of(Some(&owned(None, 0)), Some(0));
    assert_eq!(status, "True");
}

#[test]
fn a_refusal_of_the_replicaset_gives_pods_refused_with_the_refusal_in_the_message() {
    let refusal = "pods \"warm-x\" is forbidden: exceeded quota";
    let (status, reason, message) = ready_of(Some(&owned(Some(refusal), 0)), Some(2));
    assert_eq!((status.as_str(), reason.as_str()), ("False", "PodsRefused"));
    assert_eq!(message, refusal);
}

#[test]
fn enough_ready_pods_stay_ready_even_when_a_refusal_shows() {
    let (status, _, _) = ready_of(Some(&owned(Some("refused"), 3)), Some(2));
    assert_eq!(status, "True");
}

#[test]
fn an_absent_or_not_owned_replicaset_is_never_ready() {
    for (view, reason) in [
        (ReplicaSetView::Absent, "ReplicaSetAbsent"),
        (ReplicaSetView::NotOwned, "ReplicaSetNotOwned"),
    ] {
        let observation = Observation::new(Counts::new(0, 0, 9, 0), view, judgement(false), at(1));
        let (status, got, _) = ready_of(Some(&observation), Some(0));
        assert_eq!((status.as_str(), got.as_str()), ("False", reason));
    }
}

#[test]
fn a_view_that_is_not_synced_gives_unknown_not_observed_for_ready_and_accepted() {
    let status = status_of(&Acceptance::NotObserved, None, None, at(600));
    for kind in ["Accepted", "Ready"] {
        let condition = condition(&status, kind);
        assert_eq!(condition.status, "Unknown", "{kind}");
        assert_eq!(condition.reason, "NotObserved", "{kind}");
    }
}

#[test]
fn an_unknown_minimum_is_never_turned_into_a_true_ready() {
    let (status, reason, _) = ready_of(Some(&owned(None, 9)), None);
    assert_eq!(
        (status.as_str(), reason.as_str()),
        ("Unknown", "NotObserved")
    );
}

#[test]
fn with_no_observation_and_no_earlier_status_no_count_and_no_instant_is_said() {
    let status = status_of(&Acceptance::NotObserved, None, None, at(600));
    assert_eq!(status.observed_generation, None);
    assert_eq!(status.desired_replicas, None);
    assert_eq!(status.replicas, None);
    assert_eq!(status.ready_replicas, None);
    assert_eq!(status.updated_replicas, None);
    assert_eq!(status.observed_at, None);
    assert!(
        status
            .conditions
            .iter()
            .all(|c| c.type_ != "WarmPodsFailing")
    );
}

#[test]
fn a_view_that_is_lost_keeps_the_earlier_counts_the_instant_and_the_observed_generation() {
    let observation = observed(2, 500);
    let first = status_of(&Acceptance::Valid, Some(&observation), None, at(600));
    let lost = compute_status(&StatusInput::new(
        Some(8),
        Some(2),
        &Acceptance::NotObserved,
        None,
        Some(&first),
        at(700),
    ));
    assert_eq!(lost.observed_generation, Some(7));
    assert_eq!(lost.ready_replicas, Some(2));
    assert_eq!(lost.observed_at, first.observed_at);
    assert_eq!(
        condition(&lost, "WarmPodsFailing"),
        condition(&first, "WarmPodsFailing")
    );
    assert_eq!(condition(&lost, "Accepted").status, "Unknown");
}

#[test]
fn a_refused_generation_is_the_observed_generation() {
    let observation = observed(2, 500);
    let status = compute_status(&StatusInput::new(
        Some(9),
        None,
        &Acceptance::InvalidSpec("image: it is blank".to_owned()),
        Some(&observation),
        None,
        at(600),
    ));
    assert_eq!(status.observed_generation, Some(9));
    assert_eq!(condition(&status, "Accepted").message, "image: it is blank");
    assert_eq!(condition(&status, "Accepted").observed_generation, Some(9));
}

#[test]
fn the_failing_condition_of_the_resource_equals_the_one_of_the_replicaset() {
    for failing in [false, true] {
        let judged = judgement(failing);
        let observation = Observation::new(
            Counts::new(1, 1, 0, 1),
            ReplicaSetView::Owned { refusal: None },
            judged.clone(),
            at(1),
        );
        let status = status_of(&Acceptance::Valid, Some(&observation), None, at(5));
        let on_resource = condition(&status, "WarmPodsFailing");
        let on_replicaset = condition_write(&[], &judged, at(5)).unwrap();
        assert_eq!(on_resource.status, on_replicaset.status);
        assert_eq!(on_resource.reason, on_replicaset.reason);
        assert_eq!(on_resource.message, on_replicaset.message);
    }
}

#[test]
fn the_transition_time_changes_only_when_the_status_of_a_condition_changes() {
    let healthy = observed(2, 100);
    let first = status_of(&Acceptance::Valid, Some(&healthy), None, at(1000));
    let same = status_of(
        &Acceptance::Valid,
        Some(&observed(3, 150)),
        Some(&first),
        at(2000),
    );
    for kind in ["Accepted", "Ready", "WarmPodsFailing"] {
        assert_eq!(
            condition(&same, kind).last_transition_time,
            condition(&first, kind).last_transition_time,
            "{kind}"
        );
    }
    let failing = Observation::new(
        Counts::new(4, 5, 0, 3),
        ReplicaSetView::Owned { refusal: None },
        judgement(true),
        at(200),
    );
    let changed = status_of(&Acceptance::Valid, Some(&failing), Some(&same), at(3000));
    assert_eq!(
        crate::convert::timestamp_of(&condition(&changed, "WarmPodsFailing").last_transition_time),
        at(3000)
    );
    assert_eq!(
        crate::convert::timestamp_of(&condition(&changed, "Ready").last_transition_time),
        at(3000)
    );
    assert_eq!(
        condition(&changed, "Accepted").last_transition_time,
        condition(&first, "Accepted").last_transition_time
    );
}
