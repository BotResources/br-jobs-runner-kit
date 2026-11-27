use super::*;

#[test]
fn a_pending_warm_pod_that_is_not_failing_raises_the_alarm_after_the_pending_time() {
    let pod = PodFacts {
        phase: Phase::Pending,
        ready: false,
        containers: vec![waiting("ContainerCreating")],
        ..warm("p", 1000)
    };
    let early = judge(&pod, 1059);
    assert_eq!(early.action, Action::Keep);
    assert_eq!(early.recheck_at, Some(at(1060)));
    let late = judge(&pod, 1060);
    assert_eq!(late.action, Action::PendingAlarm);
    assert_eq!(late.recheck_at, Some(at(2800)));
}

#[test]
fn a_pending_warm_pod_that_is_failing_gets_no_alarm() {
    let pod = PodFacts {
        phase: Phase::Pending,
        ready: false,
        containers: vec![waiting("ImagePullBackOff")],
        ..warm("p", 1000)
    };
    assert_eq!(judge(&pod, 1500).action, Action::Keep);
    let crashing = PodFacts {
        containers: vec![terminated(1, "Error")],
        ..pod
    };
    assert_eq!(judge(&crashing, 1500).action, Action::Keep);
}

#[test]
fn a_running_warm_pod_gets_no_alarm() {
    assert_eq!(judge(&warm("w", 1000), 1500).action, Action::Keep);
}

#[test]
fn a_pod_that_is_being_deleted_is_left_alone() {
    let pod = PodFacts {
        deleting: true,
        ..warm("w", 0)
    };
    assert_eq!(
        judge(&pod, 99_999),
        Judgment {
            action: Action::Keep,
            recheck_at: None
        }
    );
}

#[test]
fn an_annotation_that_the_rule_needs_and_is_missing_or_unreadable_is_never_deleted_for() {
    let cases: Vec<(PodFacts, &str)> = vec![
        (
            PodFacts {
                clean: Mark::Absent,
                ..claimed("a", true, 0)
            },
            "clean",
        ),
        (
            PodFacts {
                clean: Mark::Unreadable,
                ..claimed("a", true, 0)
            },
            "clean",
        ),
        (
            PodFacts {
                claimed_at: Mark::Absent,
                ..claimed("a", false, 0)
            },
            "claimed-at",
        ),
        (
            PodFacts {
                claimed_at: Mark::Unreadable,
                ..claimed("a", false, 0)
            },
            "claimed-at",
        ),
        (
            PodFacts {
                claimed_at: Mark::Unreadable,
                ..claimed("a", true, 0)
            },
            "claimed-at",
        ),
        (
            PodFacts {
                idle_since: Mark::Unreadable,
                ..claimed("a", true, 0)
            },
            "idle-since",
        ),
        (
            PodFacts {
                state_bytes: Mark::Absent,
                ..saved("a", 0, 0)
            },
            "state-bytes",
        ),
        (
            PodFacts {
                state_bytes: Mark::Unreadable,
                ..saved("a", 0, 0)
            },
            "state-bytes",
        ),
    ];
    for (pod, field) in cases {
        let judged = judge(&pod, 1_000_000_000);
        assert_eq!(judged.action, Action::Unreadable(field), "{field}");
        assert_eq!(judged.recheck_at, None);
    }
}

#[test]
fn a_clean_pod_whose_idle_since_is_present_does_not_need_claimed_at() {
    let pod = PodFacts {
        claimed_at: Mark::Unreadable,
        ..saved("a", 5000, 50_000_000)
    };
    assert_eq!(judge(&pod, 5120).action, Action::Delete(DeleteReason::Idle));
}

#[test]
fn each_reason_has_the_label_of_the_metric() {
    let labels: Vec<_> = [
        DeleteReason::WarmMaxAge,
        DeleteReason::Idle,
        DeleteReason::Stuck,
        DeleteReason::Failed,
    ]
    .map(DeleteReason::label)
    .to_vec();
    assert_eq!(labels, ["warm_max_age", "idle", "stuck", "failed"]);
}
