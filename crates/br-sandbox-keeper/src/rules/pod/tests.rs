use super::*;
use crate::facts::fixtures::{claimed, terminated, waiting, warm};
use crate::time::at;

fn rules() -> ExpiryRules {
    ExpiryRules {
        warm_max_age: Duration::from_secs(1800),
        stuck_ttl: Duration::from_secs(7200),
        failed_pod_ttl: Duration::from_secs(300),
        pending_alarm: Duration::from_secs(60),
        idle_ttl: Duration::from_secs(120),
        idle_keep_min_state_bytes: 10_000_000,
    }
}

fn judge(pod: &PodFacts, now: u64) -> Judgment {
    judge_pod(pod, &rules(), None, at(now))
}

fn saved(name: &str, idle_since: u64, bytes: u64) -> PodFacts {
    PodFacts {
        idle_since: Mark::Value(at(idle_since)),
        state_bytes: Mark::Value(bytes),
        ..claimed(name, true, 0)
    }
}

#[test]
fn a_warm_pod_is_deleted_at_creation_plus_the_maximum_age() {
    let pod = warm("w", 1000);
    assert_eq!(judge(&pod, 2799).action, Action::Keep);
    assert_eq!(judge(&pod, 2799).recheck_at, Some(at(2800)));
    assert_eq!(
        judge(&pod, 2800).action,
        Action::Delete(DeleteReason::WarmMaxAge)
    );
    assert_eq!(
        judge(&pod, 9000).action,
        Action::Delete(DeleteReason::WarmMaxAge)
    );
}

#[test]
fn a_clean_pod_with_a_big_state_stays_for_the_idle_ttl() {
    let pod = saved("c", 5000, 10_000_000);
    assert_eq!(judge(&pod, 5119).action, Action::Keep);
    assert_eq!(judge(&pod, 5119).recheck_at, Some(at(5120)));
    assert_eq!(judge(&pod, 5120).action, Action::Delete(DeleteReason::Idle));
}

#[test]
fn a_clean_pod_with_a_small_state_is_deleted_at_once() {
    let pod = saved("c", 5000, 9_999_999);
    assert_eq!(judge(&pod, 5000).action, Action::Delete(DeleteReason::Idle));
    let empty = saved("c", 5000, 0);
    assert_eq!(
        judge(&empty, 5000).action,
        Action::Delete(DeleteReason::Idle)
    );
}

#[test]
fn the_idle_ttl_and_the_threshold_are_the_ones_of_the_pool() {
    let mut custom = rules();
    custom.idle_ttl = Duration::ZERO;
    let pod = saved("c", 5000, 50_000_000);
    assert_eq!(
        judge_pod(&pod, &custom, None, at(5000)).action,
        Action::Delete(DeleteReason::Idle)
    );
    custom.idle_ttl = Duration::from_secs(30);
    custom.idle_keep_min_state_bytes = 100;
    let small = saved("c", 5000, 99);
    assert_eq!(
        judge_pod(&small, &custom, None, at(5000)).action,
        Action::Delete(DeleteReason::Idle)
    );
    let big = saved("c", 5000, 100);
    assert_eq!(
        judge_pod(&big, &custom, None, at(5029)).action,
        Action::Keep
    );
    assert_eq!(
        judge_pod(&big, &custom, None, at(5030)).action,
        Action::Delete(DeleteReason::Idle)
    );
}

#[test]
fn a_clean_pod_with_no_idle_since_is_deleted_at_claimed_at_plus_the_stuck_ttl() {
    let pod = claimed("c", true, 1000);
    assert_eq!(judge(&pod, 8199).action, Action::Keep);
    assert_eq!(judge(&pod, 8199).recheck_at, Some(at(8200)));
    assert_eq!(
        judge(&pod, 8200).action,
        Action::Delete(DeleteReason::Stuck)
    );
}

#[test]
fn a_dirty_pod_is_deleted_at_claimed_at_plus_the_stuck_ttl_and_not_before() {
    let pod = claimed("d", false, 1000);
    assert_eq!(judge(&pod, 1001).action, Action::Keep);
    assert_eq!(judge(&pod, 8199).action, Action::Keep);
    assert_eq!(
        judge(&pod, 8200).action,
        Action::Delete(DeleteReason::Stuck)
    );
}

#[test]
fn a_dirty_pod_ignores_an_idle_since_left_from_an_earlier_release() {
    let pod = PodFacts {
        idle_since: Mark::Value(at(10)),
        state_bytes: Mark::Value(0),
        ..claimed("d", false, 1000)
    };
    assert_eq!(judge(&pod, 1500).action, Action::Keep);
}

#[test]
fn a_failed_pod_is_deleted_failed_pod_ttl_after_it_was_first_seen() {
    let pod = PodFacts {
        phase: Phase::Failed,
        ..warm("f", 0)
    };
    let seen = Some(at(4000));
    assert_eq!(
        judge_pod(&pod, &rules(), seen, at(4299)).action,
        Action::Keep
    );
    assert_eq!(
        judge_pod(&pod, &rules(), seen, at(4299)).recheck_at,
        Some(at(4300))
    );
    assert_eq!(
        judge_pod(&pod, &rules(), seen, at(4300)).action,
        Action::Delete(DeleteReason::Failed)
    );
}

#[test]
fn a_failed_pod_not_seen_before_gets_the_age_from_now() {
    let pod = PodFacts {
        phase: Phase::Failed,
        ..claimed("f", false, 0)
    };
    let judged = judge_pod(&pod, &rules(), None, at(4000));
    assert_eq!(judged.action, Action::Keep);
    assert_eq!(judged.recheck_at, Some(at(4300)));
}

#[test]
fn a_claimed_pod_in_the_failed_phase_follows_the_failed_rule_not_the_stuck_rule() {
    let pod = PodFacts {
        phase: Phase::Failed,
        ..claimed("f", false, 0)
    };
    assert_eq!(
        judge_pod(&pod, &rules(), Some(at(0)), at(300)).action,
        Action::Delete(DeleteReason::Failed)
    );
}

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
fn a_pending_warm_pod_older_than_the_maximum_age_is_deleted_not_alarmed() {
    let pod = PodFacts {
        phase: Phase::Pending,
        ready: false,
        ..warm("p", 1000)
    };
    assert_eq!(
        judge(&pod, 2800).action,
        Action::Delete(DeleteReason::WarmMaxAge)
    );
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
