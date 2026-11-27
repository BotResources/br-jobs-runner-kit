use crate::facts::{ConditionFacts, Phase, PodFacts, fixtures};

use super::*;

fn conditions(bed: &Bed) -> Vec<(String, String)> {
    bed.kube
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Condition { status, reason, .. } => Some((status, reason)),
            _ => None,
        })
        .collect()
}

fn failing(name: &str, reason: &str) -> PodFacts {
    PodFacts {
        phase: Phase::Pending,
        ready: false,
        containers: vec![fixtures::waiting(reason)],
        ..old_warm(name, 10)
    }
}

fn held(status: &str, reason: &str, message: &str) -> ConditionFacts {
    ConditionFacts {
        kind: "WarmPodsFailing".to_owned(),
        status: status.to_owned(),
        reason: Some(reason.to_owned()),
        message: Some(message.to_owned()),
        last_transition_time: Some(at(START)),
    }
}

#[tokio::test(start_paused = true)]
async fn the_first_judgement_of_a_pool_writes_false_and_the_next_ones_write_nothing() {
    let mut bed = bed(true);
    bed.kube.put_pod(old_warm("w0", 10));
    bed.sync().await;
    assert_eq!(
        conditions(&bed),
        vec![("False".to_owned(), "NoWarmPodFailing".to_owned())]
    );
    bed.set_pool_conditions(vec![held("False", "NoWarmPodFailing", "")]);
    bed.kube.clear_calls();
    bed.pass(30).await;
    assert!(conditions(&bed).is_empty());
}

#[tokio::test(start_paused = true)]
async fn the_condition_goes_true_with_the_pull_reason_then_false_when_no_pod_fails() {
    let mut bed = bed(true);
    bed.kube.put_pod(failing("w0", "ErrImagePull"));
    bed.sync().await;
    assert_eq!(
        conditions(&bed),
        vec![("True".to_owned(), "ImagePullFailing".to_owned())]
    );
    bed.set_pool_conditions(vec![held(
        "True",
        "ImagePullFailing",
        "1 warm pod(s) failing; kubelet reason: ErrImagePull",
    )]);
    bed.kube.clear_calls();
    bed.kube.update_pod(NS, "w0", |pod| {
        pod.ready = true;
        pod.phase = Phase::Running;
        pod.containers.clear();
    });
    bed.settle().await;
    assert_eq!(
        conditions(&bed),
        vec![("False".to_owned(), "NoWarmPodFailing".to_owned())]
    );
}

#[tokio::test(start_paused = true)]
async fn a_pull_failure_wins_over_a_crash_and_the_failing_pods_are_not_deleted() {
    let mut bed = bed(true);
    bed.kube.put_pod(failing("a", "CrashLoopBackOff"));
    bed.kube.put_pod(failing("b", "ImagePullBackOff"));
    bed.sync().await;
    let written = conditions(&bed);
    assert_eq!(written.last().unwrap().1, "ImagePullFailing");
    assert!(bed.deletes().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_follower_writes_no_condition() {
    let mut bed = bed(false);
    bed.kube.put_pod(failing("a", "ErrImagePull"));
    bed.sync().await;
    assert!(conditions(&bed).is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_failed_write_of_the_condition_is_counted_and_tried_again() {
    let mut bed = bed(true);
    bed.kube.put_pod(old_warm("w0", 10));
    bed.kube.fail("apply_condition");
    bed.sync().await;
    assert!(bed.errors() >= 1);
    assert!(bed.rs_conditions().is_empty());
    bed.kube.heal("apply_condition");
    bed.pass(3).await;
    assert_eq!(
        bed.rs_conditions()
            .iter()
            .map(|condition| condition.status.as_str())
            .collect::<Vec<_>>(),
        vec!["False"]
    );
}

#[tokio::test(start_paused = true)]
async fn the_condition_of_the_replicaset_and_the_one_of_the_resource_are_the_same() {
    let mut bed = bed(true);
    bed.kube.put_pod(failing("a", "ErrImagePull"));
    bed.sync().await;
    let on_set = bed.rs_conditions();
    assert_eq!(on_set.len(), 1);
    let status = bed.status().unwrap();
    let on_resource = status["conditions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|condition| condition["type"] == "WarmPodsFailing")
        .unwrap();
    assert_eq!(on_resource["status"], on_set[0].status.as_str());
    assert_eq!(on_resource["reason"], on_set[0].reason.as_deref().unwrap());
    assert_eq!(
        on_resource["message"],
        on_set[0].message.as_deref().unwrap()
    );
}
