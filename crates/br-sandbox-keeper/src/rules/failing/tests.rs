use super::*;
use crate::facts::fixtures::{terminated, waiting, warm};
use crate::time::at;

fn pending(name: &str, containers: Vec<ContainerFacts>) -> PodFacts {
    PodFacts {
        ready: false,
        containers,
        ..warm(name, 0)
    }
}

#[test]
fn each_pull_reason_of_the_kubelet_is_a_pull_failure() {
    for reason in ["ErrImagePull", "ImagePullBackOff", "InvalidImageName"] {
        let failure = pod_failure(&pending("a", vec![waiting(reason)])).unwrap();
        assert_eq!(failure.kind, FailureKind::ImagePull);
        assert_eq!(failure.kubelet_reason, reason);
    }
}

#[test]
fn a_crash_loop_and_a_non_zero_exit_are_crashes_at_start() {
    let looping = pod_failure(&pending("a", vec![waiting("CrashLoopBackOff")])).unwrap();
    assert_eq!(looping.kind, FailureKind::Crash);
    let exited = pod_failure(&pending("a", vec![terminated(3, "OOMKilled")])).unwrap();
    assert_eq!(exited.kind, FailureKind::Crash);
    assert_eq!(exited.kubelet_reason, "OOMKilled");
}

#[test]
fn a_container_that_ended_with_exit_code_0_is_not_failing() {
    assert_eq!(
        pod_failure(&pending("a", vec![terminated(0, "Completed")])),
        None
    );
}

#[test]
fn a_container_that_waits_for_another_reason_is_not_failing() {
    for reason in [
        "ContainerCreating",
        "PodInitializing",
        "CreateContainerConfigError",
    ] {
        assert_eq!(
            pod_failure(&pending("a", vec![waiting(reason)])),
            None,
            "{reason}"
        );
    }
}

#[test]
fn a_pod_that_is_ready_is_never_failing() {
    let mut pod = pending("a", vec![waiting("CrashLoopBackOff")]);
    pod.ready = true;
    assert_eq!(pod_failure(&pod), None);
}

#[test]
fn a_pod_that_is_being_deleted_is_not_failing() {
    let mut pod = pending("a", vec![terminated(143, "Error")]);
    pod.deleting = true;
    assert_eq!(pod_failure(&pod), None);
}

#[test]
fn an_init_container_counts_like_a_main_container() {
    let pod = pending(
        "a",
        vec![terminated(0, "Completed"), waiting("ImagePullBackOff")],
    );
    assert_eq!(pod_failure(&pod).unwrap().kind, FailureKind::ImagePull);
}

#[test]
fn a_pod_with_both_causes_reports_the_pull_failure() {
    let pod = pending(
        "a",
        vec![waiting("CrashLoopBackOff"), waiting("ErrImagePull")],
    );
    let failure = pod_failure(&pod).unwrap();
    assert_eq!(failure.kind, FailureKind::ImagePull);
    assert_eq!(failure.kubelet_reason, "ErrImagePull");
}

#[test]
fn a_pool_with_no_failing_pod_is_judged_not_failing() {
    let pods = [
        warm("a", 0),
        pending("b", vec![waiting("ContainerCreating")]),
    ];
    let judged = judge_pool(&pods);
    assert!(!judged.failing);
    assert_eq!(judged.message, "");
    assert!(!judge_pool(std::iter::empty()).failing);
}

#[test]
fn a_pool_with_a_pull_failure_reports_it_with_the_count_and_the_kubelet_reason() {
    let pods = [
        pending("b", vec![waiting("ImagePullBackOff")]),
        pending("a", vec![waiting("ErrImagePull")]),
        warm("c", 0),
    ];
    let judged = judge_pool(&pods);
    assert!(judged.failing);
    assert_eq!(judged.reason, "ImagePullFailing");
    assert_eq!(
        judged.message,
        "2 warm pod(s) failing; kubelet reason: ErrImagePull"
    );
}

#[test]
fn a_pool_with_both_causes_reports_the_pull_failure() {
    let pods = [
        pending("a", vec![waiting("CrashLoopBackOff")]),
        pending("b", vec![waiting("ImagePullBackOff")]),
    ];
    let judged = judge_pool(&pods);
    assert_eq!(judged.reason, "ImagePullFailing");
    assert!(judged.message.starts_with("2 warm pod(s)"));
}

#[test]
fn a_pool_with_a_crash_only_reports_start_crashing() {
    let judged = judge_pool(&[pending("a", vec![terminated(1, "Error")])]);
    assert_eq!(judged.reason, "StartCrashing");
}

#[test]
fn the_message_holds_no_text_of_the_kubelet_but_its_reason() {
    let pod = pending("a", vec![waiting("ImagePullBackOff")]);
    let judged = judge_pool(&[pod]);
    assert_eq!(
        judged.message,
        "1 warm pod(s) failing; kubelet reason: ImagePullBackOff"
    );
}

fn judged(failing: bool) -> PoolJudgement {
    if failing {
        PoolJudgement {
            failing,
            reason: "ImagePullFailing",
            message: "1 warm pod(s) failing; kubelet reason: ErrImagePull".to_owned(),
        }
    } else {
        PoolJudgement {
            failing,
            reason: "NoWarmPodFailing",
            message: String::new(),
        }
    }
}

fn existing(status: &str, reason: &str, message: &str, at_second: u64) -> ConditionFacts {
    ConditionFacts {
        kind: "WarmPodsFailing".to_owned(),
        status: status.to_owned(),
        reason: Some(reason.to_owned()),
        message: Some(message.to_owned()),
        last_transition_time: Some(at(at_second)),
    }
}

#[test]
fn a_pool_not_yet_judged_gets_the_condition_false() {
    let write = condition_write(&[], &judged(false), at(500)).unwrap();
    assert_eq!(write.status, "False");
    assert_eq!(write.last_transition_time, at(500));
}

#[test]
fn another_condition_does_not_count_as_judged() {
    let other = ConditionFacts {
        kind: "ReplicaFailure".to_owned(),
        ..existing("False", "x", "", 1)
    };
    assert!(condition_write(&[other], &judged(false), at(500)).is_some());
}

#[test]
fn a_change_of_status_changes_the_transition_time() {
    let current = [existing("False", "NoWarmPodFailing", "", 100)];
    let write = condition_write(&current, &judged(true), at(500)).unwrap();
    assert_eq!(write.status, "True");
    assert_eq!(write.last_transition_time, at(500));
    let back = [existing("True", "ImagePullFailing", "m", 100)];
    let write = condition_write(&back, &judged(false), at(900)).unwrap();
    assert_eq!(write.last_transition_time, at(900));
}

#[test]
fn a_new_reason_or_message_with_the_same_status_keeps_the_transition_time() {
    let current = [existing(
        "True",
        "StartCrashing",
        "1 warm pod(s) failing; kubelet reason: Error",
        100,
    )];
    let write = condition_write(&current, &judged(true), at(500)).unwrap();
    assert_eq!(write.reason, "ImagePullFailing");
    assert_eq!(write.last_transition_time, at(100));
}

#[test]
fn a_condition_that_is_already_as_judged_is_not_written() {
    let same = [existing(
        "True",
        "ImagePullFailing",
        &judged(true).message,
        100,
    )];
    assert_eq!(condition_write(&same, &judged(true), at(500)), None);
    let none = [existing("False", "NoWarmPodFailing", "", 100)];
    assert_eq!(condition_write(&none, &judged(false), at(500)), None);
}

#[test]
fn a_pod_that_has_been_ready_is_not_failing_when_a_container_crashes_later() {
    let restarted = PodFacts {
        been_ready: true,
        ..pending("a", vec![waiting("CrashLoopBackOff")])
    };
    assert_eq!(pod_failure(&restarted), None);
    let never_ready = pending("a", vec![waiting("CrashLoopBackOff")]);
    assert!(pod_failure(&never_ready).is_some());
}
