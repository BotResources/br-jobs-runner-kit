use std::time::{Duration, UNIX_EPOCH};

use br_runner_sandbox::Timestamp;
use br_sandbox_keeper::bucket::ObjectInfo;
use br_sandbox_keeper::cluster::{EventTarget, LeaseWrite};
use br_sandbox_keeper::facts::{
    ConditionFacts, ContainerFacts, Mark, Phase, PodFacts, PodVersion, PoolState, ReplicaSetFacts,
    Terminated,
};
use br_sandbox_keeper::rules::{
    Action, DeleteReason, ExpiryRules, ScaleDownRules, ScaleState, SweepRules, condition_write,
    damp, judge_pod, judge_pool, record_needs_read,
};

fn at(seconds: u64) -> Timestamp {
    Timestamp::from_system_time(UNIX_EPOCH + Duration::from_secs(seconds))
}

fn seconds(count: u64) -> Duration {
    Duration::from_secs(count)
}

#[test]
fn an_importer_builds_the_pod_and_the_rules_with_the_constructors_and_applies_the_rules() {
    let rules = ExpiryRules::new(
        seconds(1800),
        seconds(7200),
        seconds(300),
        seconds(60),
        seconds(120),
        10_000_000,
    );
    let mut pod = PodFacts::new("sbx", "w0", "uid-w0", "1", PoolState::Warm, at(0));
    pod.phase = Phase::Running;
    pod.ready = true;
    assert_eq!(
        judge_pod(&pod, &rules, None, at(1800)).action,
        Action::Delete(DeleteReason::WarmMaxAge)
    );
    assert_eq!(pod.version(), PodVersion::new("uid-w0", "1"));
    assert_eq!(pod.clean, Mark::Absent);

    let mut crashing = PodFacts::new("sbx", "w1", "uid-w1", "2", PoolState::Warm, at(0));
    crashing.containers = vec![
        ContainerFacts::waiting("ErrImagePull"),
        ContainerFacts::terminated(Terminated::new(1, None)),
    ];
    let judged = judge_pool([&crashing]);
    assert!(judged.failing);
    let write = condition_write(
        &[ConditionFacts::new("WarmPodsFailing", "False")],
        &judged,
        at(10),
    );
    assert!(write.is_some());
}

#[test]
fn an_importer_builds_the_other_inputs_with_the_constructors() {
    let damped = damp(
        ScaleState::default(),
        10,
        3,
        at(0),
        &ScaleDownRules::new(seconds(120), 5, seconds(10)),
    );
    assert_eq!(damped.replicas, 10);
    let sweep = SweepRules::new(seconds(1000), seconds(60), seconds(3600), seconds(10_000));
    assert!(record_needs_read(
        &ObjectInfo::new("records/x", at(1)),
        at(1),
        &sweep
    ));
    let _ = (
        ReplicaSetFacts::new("rs", Some(3)),
        EventTarget::new("warm-p", "rs"),
        LeaseWrite::new("a", at(1), at(2), seconds(15), 0),
    );
}

#[test]
fn the_enums_of_the_read_model_can_grow_so_an_importer_matches_them_with_a_wildcard() {
    let phase = match Phase::Pending {
        Phase::Pending | Phase::Running => "live",
        _ => "other",
    };
    assert_eq!(phase, "live");
    let mark = match Mark::Value(1_u8) {
        Mark::Value(value) => i32::from(value),
        _ => -1,
    };
    assert_eq!(mark, 1);
    let state = match PoolState::Warm {
        PoolState::Warm => "warm",
        _ => "other",
    };
    assert_eq!(state, "warm");
}
