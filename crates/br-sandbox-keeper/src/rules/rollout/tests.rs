use super::*;
use crate::facts::fixtures::{claimed, terminated, waiting, warm};

const CURRENT: &str = "hash-new";
const OLD: &str = "hash-old";

fn pod(name: &str, hash: &str, created: u64) -> PodFacts {
    PodFacts {
        template_hash: Mark::Value(hash.to_owned()),
        ..warm(name, created)
    }
}

fn stale(name: &str, created: u64) -> PodFacts {
    pod(name, OLD, created)
}

fn fresh(name: &str, created: u64) -> PodFacts {
    pod(name, CURRENT, created)
}

fn not_ready(pod: PodFacts) -> PodFacts {
    PodFacts {
        ready: false,
        phase: Phase::Pending,
        ..pod
    }
}

fn run(pods: &[PodFacts], step: u32) -> RolloutPlan {
    plan_rollout(pods, CURRENT, step, u32::try_from(pods.len()).unwrap())
}

#[test]
fn a_pool_with_no_stale_pod_has_nothing_to_do() {
    let plan = run(&[fresh("a", 1), fresh("b", 2)], 5);
    assert_eq!(plan.state, RolloutState::Settled);
    assert!(plan.delete.is_empty());
    assert_eq!((plan.stale, plan.updated), (0, 2));
}

#[test]
fn a_step_removes_at_most_scale_down_step_stale_pods_and_the_oldest_go_first() {
    let pods: Vec<PodFacts> = (0..8).map(|n| stale(&format!("s{n}"), 100 + n)).collect();
    let plan = run(&pods, 3);
    assert_eq!(plan.state, RolloutState::Step);
    assert_eq!(plan.delete, ["s0", "s1", "s2"]);
    assert_eq!((plan.stale, plan.updated), (8, 0));
    assert_eq!(run(&pods[..2], 5).delete, ["s0", "s1"]);
}

#[test]
fn the_pods_that_are_not_ready_go_before_the_ready_ones_whatever_their_age() {
    let pods = vec![
        stale("old-ready", 1),
        not_ready(stale("young-not-ready", 900)),
        stale("mid-ready", 50),
        not_ready(stale("old-not-ready", 10)),
    ];
    assert_eq!(run(&pods, 3).delete, ["old-not-ready", "young-not-ready", "old-ready"]);
}

#[test]
fn a_stale_pod_that_cannot_be_scheduled_is_taken_by_a_step_like_a_pod_that_is_not_ready() {
    let held = PodFacts {
        unschedulable: true,
        ..not_ready(stale("held", 500))
    };
    let pods = vec![stale("ready", 1), held];
    assert_eq!(run(&pods, 1).delete, ["held"]);
}

#[test]
fn the_next_step_waits_until_every_pod_of_the_current_template_is_ready() {
    let pods = vec![stale("s", 1), not_ready(fresh("n", 5))];
    let plan = run(&pods, 5);
    assert_eq!(plan.state, RolloutState::Waiting(Wait::CurrentPodNotReady));
    assert!(plan.delete.is_empty());
    let ready = vec![stale("s", 1), fresh("n", 5)];
    assert_eq!(run(&ready, 5).delete, ["s"]);
}

#[test]
fn a_pod_of_the_current_template_that_waits_for_a_place_makes_the_rollout_wait() {
    let waiting_for_a_place = PodFacts {
        unschedulable: true,
        ..not_ready(fresh("n", 5))
    };
    let plan = run(&[stale("s", 1), waiting_for_a_place], 5);
    assert_eq!(plan.state, RolloutState::Waiting(Wait::CurrentPodNotReady));
}

#[test]
fn while_a_pod_of_the_current_template_is_failing_the_rollout_stops_and_removes_nothing() {
    for container in [
        waiting("ImagePullBackOff"),
        waiting("ErrImagePull"),
        waiting("InvalidImageName"),
        waiting("CrashLoopBackOff"),
        terminated(2, "Error"),
    ] {
        let failing = PodFacts {
            containers: vec![container],
            ..not_ready(fresh("bad", 5))
        };
        let pods = vec![stale("s1", 1), stale("s2", 2), failing];
        let plan = run(&pods, 5);
        assert_eq!(plan.state, RolloutState::Stopped);
        assert!(plan.delete.is_empty());
        assert_eq!(plan.stale, 2);
    }
}

#[test]
fn a_failing_stale_pod_does_not_stop_the_rollout() {
    let failing = PodFacts {
        containers: vec![waiting("ImagePullBackOff")],
        ..not_ready(stale("bad-old", 5))
    };
    let plan = run(&[failing, stale("s", 1)], 5);
    assert_eq!(plan.state, RolloutState::Step);
    assert_eq!(plan.delete, ["bad-old", "s"]);
}

#[test]
fn the_rollout_goes_on_when_no_pod_of_the_current_template_is_failing_and_all_are_ready() {
    let pods = vec![stale("s", 1), fresh("ok", 5)];
    assert_eq!(run(&pods, 5).state, RolloutState::Step);
}

#[test]
fn a_claimed_pod_is_never_in_a_step_and_is_not_counted() {
    let pods = vec![
        PodFacts {
            template_hash: Mark::Value(OLD.to_owned()),
            ..claimed("busy", false, 10)
        },
        stale("s", 1),
    ];
    let plan = plan_rollout(&pods, CURRENT, 5, 1);
    assert_eq!(plan.delete, ["s"]);
    assert_eq!(plan.stale, 1);
}

#[test]
fn a_pod_of_the_current_template_is_never_stale_and_never_deleted() {
    let pods: Vec<PodFacts> = (0..4).map(|n| fresh(&format!("f{n}"), n)).collect();
    let plan = run(&pods, 5);
    assert!(plan.delete.is_empty());
    assert_eq!(plan.updated, 4);
}

#[test]
fn a_warm_pod_with_no_hash_or_an_unreadable_hash_is_never_stale_and_is_reported() {
    let no_hash = warm("no-hash", 1);
    let bad_hash = PodFacts {
        template_hash: Mark::Unreadable,
        ..warm("bad-hash", 2)
    };
    let pods = vec![no_hash, bad_hash, stale("s", 3)];
    let plan = run(&pods, 5);
    assert_eq!(plan.delete, ["s"]);
    assert_eq!(plan.unreadable, ["bad-hash", "no-hash"]);
    assert_eq!(plan.stale, 1);
    let only_unreadable = vec![warm("no-hash", 1)];
    assert_eq!(run(&only_unreadable, 5).state, RolloutState::Settled);
}

#[test]
fn a_pod_that_is_being_deleted_is_not_counted_and_is_not_chosen_again() {
    let leaving = PodFacts {
        deleting: true,
        ..stale("leaving", 1)
    };
    let plan = plan_rollout(&[leaving, stale("s", 2)], CURRENT, 5, 1);
    assert_eq!(plan.delete, ["s"]);
    assert_eq!(plan.stale, 1);
}

#[test]
fn no_new_step_starts_while_the_replacements_of_the_last_step_are_not_there_yet() {
    let pods = vec![stale("s1", 1), stale("s2", 2)];
    let plan = plan_rollout(&pods, CURRENT, 5, 5);
    assert_eq!(plan.state, RolloutState::Waiting(Wait::ReplacementsMissing));
    assert!(plan.delete.is_empty());
    assert_eq!(plan_rollout(&pods, CURRENT, 5, 2).state, RolloutState::Step);
}

#[test]
fn a_step_of_zero_removes_nothing() {
    let plan = run(&[stale("s", 1)], 0);
    assert_eq!(plan.state, RolloutState::Settled);
    assert!(plan.delete.is_empty());
}

#[test]
fn a_rollout_that_goes_well_ends_in_ceil_of_stale_over_step_steps_and_makes_no_loop() {
    let step = 5;
    let mut pods: Vec<PodFacts> = (0..20).map(|n| stale(&format!("s{n}"), n)).collect();
    let mut made = 0;
    let mut steps = 0;
    for _ in 0..50 {
        let plan = plan_rollout(&pods, CURRENT, step, 20);
        match plan.state {
            RolloutState::Settled => break,
            RolloutState::Step => {
                steps += 1;
                pods.retain(|pod| !plan.delete.contains(&pod.name));
                for _ in &plan.delete {
                    made += 1;
                    pods.push(not_ready(fresh(&format!("n{made}"), 1000 + made)));
                }
                for pod in pods.iter_mut().filter(|pod| !pod.ready) {
                    pod.ready = true;
                    pod.phase = Phase::Running;
                }
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(steps, 4);
    assert_eq!(made, 20);
    assert!(pods.iter().all(|pod| pod.template_hash == Mark::Value(CURRENT.to_owned())));
}

#[test]
fn a_typing_error_in_an_image_loses_at_most_one_step_of_pods() {
    let mut pods: Vec<PodFacts> = (0..20).map(|n| stale(&format!("s{n}"), n)).collect();
    let first = plan_rollout(&pods, CURRENT, 5, 20);
    assert_eq!(first.delete.len(), 5);
    pods.retain(|pod| !first.delete.contains(&pod.name));
    for index in 0..5 {
        pods.push(PodFacts {
            containers: vec![waiting("ImagePullBackOff")],
            ..not_ready(fresh(&format!("bad{index}"), 1000))
        });
    }
    for _ in 0..10 {
        let plan = plan_rollout(&pods, CURRENT, 5, 20);
        assert_eq!(plan.state, RolloutState::Stopped);
        assert!(plan.delete.is_empty());
    }
    assert_eq!(pods.iter().filter(|pod| pod.ready).count(), 15);
}

#[test]
fn at_the_same_age_the_name_breaks_the_tie() {
    let pods = vec![stale("b", 10), stale("a", 10)];
    assert_eq!(run(&pods, 1).delete, ["a"]);
}
