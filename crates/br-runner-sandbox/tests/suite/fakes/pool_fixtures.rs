use std::collections::BTreeMap;

use br_runner_sandbox::{ConditionView, PodKeys, PodView, ROLE_SANDBOX, ReplicaSetView};
use serde_json::{Value, json};

pub fn warm_pod(name: &str) -> PodView {
    let keys = PodKeys::default();
    PodView {
        name: name.to_owned(),
        resource_version: String::new(),
        labels: BTreeMap::from([
            (keys.role(), ROLE_SANDBOX.to_owned()),
            (keys.pool_id(), "main".to_owned()),
            (keys.pool(), "warm".to_owned()),
        ]),
        annotations: BTreeMap::new(),
        phase: Some("Running".to_owned()),
        ready_condition: true,
        deleting: false,
    }
}

pub fn pool_template() -> Value {
    let keys = PodKeys::default();
    json!({
        "metadata": {
            "labels": {
                keys.role(): ROLE_SANDBOX,
                keys.pool_id(): "main",
                keys.pool(): "warm",
                "app": "sandbox",
            },
            "annotations": {"example/note": "kept"},
        },
        "spec": {
            "runtimeClassName": "gvisor",
            "containers": [
                {"name": "sandbox", "image": "agent:1"},
                {"name": "seed", "image": "tools:1"},
            ],
        },
    })
}

pub fn pool(conditions: Vec<ConditionView>) -> ReplicaSetView {
    ReplicaSetView {
        name: "warm-main".to_owned(),
        conditions,
        template: pool_template(),
    }
}

pub fn failing(reason: &str, message: &str) -> ConditionView {
    ConditionView {
        kind: "WarmPodsFailing".to_owned(),
        status: "True".to_owned(),
        reason: Some(reason.to_owned()),
        message: Some(message.to_owned()),
    }
}
