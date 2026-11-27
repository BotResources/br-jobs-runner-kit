use std::time::{Duration, UNIX_EPOCH};

use br_runner_sandbox::{AttemptId, Record, Timestamp};
use br_sandbox_keeper::cluster::{EventTarget, LeaseWrite};
use br_sandbox_keeper::facts::{
    ConditionFacts, ContainerFacts, Mark, Phase, PodFacts, PodVersion, PoolState, ReplicaSetFacts,
    Terminated,
};
use br_sandbox_keeper::rules::{
    Action, DeleteReason, ExpiryRules, ScaleDownRules, ScaleState, SweepRules, condition_write,
    damp, judge_pod, judge_pool, record_is_old,
};
use br_sandbox_keeper::pool::{
    Acceptance, Counts, FenceFacts, FenceVerdict, Observation, PoolRef, PoolTemplate,
    ReplicaSetView, StatusInput, StatusWrite, TemplateFault, compute_status, decide_write,
    fence_verdict, is_duplicate, judge_acceptance, replicaset_name, validate_spec,
};
use br_sandbox_keeper::resource::{PoolSize, SandboxPoolSpec, SizeBounds};
use br_sandbox_keeper::rules::plan_rollout;
use br_sandbox_keeper::{ConfigError, KeeperConfig, KeeperConfigInput};

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
        judge_pod(&pod, &rules, None, false, at(1800)).action,
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
    let record = Record::started(AttemptId::mint(), at(1));
    assert_eq!(
        record_is_old(&record.to_vec().unwrap(), at(10_001), &sweep),
        Ok(true)
    );
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

#[test]
fn an_importer_builds_the_configuration_input_from_its_default_and_converts_it() {
    let mut input = KeeperConfigInput::default();
    input.store_bucket = Some("saves".to_owned());
    input.store_endpoint = Some("http://store.test:8333".to_owned());
    input.store_region = Some("eu".to_owned());
    input.listen_address = Some("127.0.0.1:0".to_owned());
    input.save_ttl_seconds = Some(1000);
    input.agent_container = Some("agent".to_owned());
    input.idle_ttl_seconds = Some(30);
    let config = KeeperConfig::try_from(input.clone()).unwrap();
    assert_eq!(config.agent_container, "agent");
    assert_eq!(config.idle_ttl, std::time::Duration::from_secs(30));
    input.pools = Some(serde_json::json!([]));
    assert_eq!(
        KeeperConfig::try_from(input).unwrap_err(),
        ConfigError::PoolsAreResources
    );
}

#[test]
fn an_importer_applies_the_pure_rules_of_the_resource_with_the_constructors() {
    let size = PoolSize::new(
        SizeBounds::new("500m", "2"),
        SizeBounds::new("1Gi", "4Gi"),
        SizeBounds::new("1Gi", "8Gi"),
    );
    let spec = SandboxPoolSpec::new("registry.example/agent:1", size, 1, 4);
    let settings = validate_spec(&spec).unwrap();
    assert_eq!(settings.max_replicas, 4);
    assert_eq!(replicaset_name("sbx"), "warm-sbx");

    let keys = br_runner_sandbox::PodKeys::default();
    assert_eq!(
        PoolTemplate::read(None, "sbx", &keys, "agent").unwrap_err(),
        TemplateFault::Absent
    );

    let labels = std::collections::BTreeMap::new();
    let facts = FenceFacts::new("sbx", &labels, "br.sandbox/managed=true", &[], &[], None);
    let verdict = fence_verdict(&facts);
    assert!(matches!(verdict, FenceVerdict::NotFenced(_)));

    let older = PoolRef::new("a", Some(at(1)));
    let newer = PoolRef::new("b", Some(at(2)));
    assert!(is_duplicate(&newer, &[older, newer.clone()]));

    let plan = plan_rollout(std::iter::empty::<&PodFacts>(), "hash", 5, 0);
    assert!(plan.delete.is_empty());

    let acceptance = judge_acceptance(Some(&verdict), Some(false), &Ok(settings), None);
    assert!(matches!(acceptance, Acceptance::NamespaceNotFenced(_)));
    let observation = Observation::new(
        Counts::new(1, 1, 1, 1),
        ReplicaSetView::Absent,
        judge_pool(std::iter::empty::<&PodFacts>()),
        at(10),
    );
    let input = StatusInput::new(Some(1), Some(1), &acceptance, Some(&observation), None, at(10));
    let status = compute_status(&input);
    assert!(matches!(
        decide_write(status, None, None, at(10)),
        StatusWrite::Write(_)
    ));
}
