use crate::facts::{Phase, PodFacts, fixtures};

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

#[tokio::test(start_paused = true)]
async fn the_first_judgement_of_a_pool_writes_false_and_the_next_ones_write_nothing() {
    let mut bed = bed(true);
    bed.kube.put_pod(old_warm("w0", 10));
    bed.sync().await;
    assert_eq!(
        conditions(&bed),
        vec![("False".to_owned(), "NoWarmPodFailing".to_owned())]
    );
    bed.kube.set_replica_set_conditions(
        NS,
        RS,
        vec![crate::facts::ConditionFacts {
            kind: "WarmPodsFailing".to_owned(),
            status: "False".to_owned(),
            reason: Some("NoWarmPodFailing".to_owned()),
            message: Some(String::new()),
            last_transition_time: Some(at(START)),
        }],
    );
    bed.kube.clear_calls();
    bed.pass(30).await;
    assert!(conditions(&bed).is_empty());
}

#[tokio::test(start_paused = true)]
async fn the_condition_goes_true_with_the_pull_reason_then_false_when_no_pod_fails() {
    let mut bed = bed(true);
    let pod = bed.kube.put_pod(failing("w0", "ErrImagePull"));
    bed.sync().await;
    let written = conditions(&bed);
    assert_eq!(
        written,
        vec![("True".to_owned(), "ImagePullFailing".to_owned())]
    );
    bed.kube.set_replica_set_conditions(
        NS,
        RS,
        vec![crate::facts::ConditionFacts {
            kind: "WarmPodsFailing".to_owned(),
            status: "True".to_owned(),
            reason: Some("ImagePullFailing".to_owned()),
            message: Some("1 warm pod(s) failing; kubelet reason: ErrImagePull".to_owned()),
            last_transition_time: Some(at(START)),
        }],
    );
    bed.kube.clear_calls();
    bed.applied(PodFacts {
        ready: true,
        phase: Phase::Running,
        containers: Vec::new(),
        ..pod
    })
    .await;
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
async fn a_failed_write_of_the_condition_is_tried_again() {
    let mut bed = bed(true);
    bed.kube.put_pod(old_warm("w0", 10));
    bed.kube.fail("apply_condition");
    bed.sync().await;
    assert!(bed.has_metric("br_sandbox_keeper_sweep_errors_total 1"));
    bed.kube.heal("apply_condition");
    bed.pass(3).await;
    assert_eq!(conditions(&bed).len(), 2);
}
