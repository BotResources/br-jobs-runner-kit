use std::collections::BTreeMap;

use br_agentic_harness_contract::{AgentId, ThreadRef, TurnId};
use serde_json::{Value, json};

use crate::config::PoolId;
use crate::keys::{CLEAN_FALSE, CLEAN_TRUE, POOL_CLAIMED, PodKeys};
use crate::time::Timestamp;

pub fn warm_claim_patch(
    keys: &PodKeys,
    resource_version: &str,
    thread: ThreadRef,
    agent: AgentId,
    now: Timestamp,
) -> Value {
    json!({
        "metadata": {
            "resourceVersion": resource_version,
            "labels": {
                keys.pool(): POOL_CLAIMED,
                keys.thread(): thread.to_string(),
                keys.agent(): agent.to_string(),
            },
            "annotations": {
                keys.clean(): CLEAN_FALSE,
                keys.claimed_at(): now.to_string(),
            },
        }
    })
}

pub fn live_claim_patch(keys: &PodKeys, resource_version: &str, now: Timestamp) -> Value {
    json!({
        "metadata": {
            "resourceVersion": resource_version,
            "annotations": {
                keys.clean(): CLEAN_FALSE,
                keys.claimed_at(): now.to_string(),
                keys.idle_since(): Value::Null,
                keys.state_bytes(): Value::Null,
            },
        }
    })
}

pub fn saved_patch(keys: &PodKeys, turn: TurnId, bytes_raw: u64, made_at: Timestamp) -> Value {
    json!({
        "metadata": {
            "annotations": {
                keys.clean(): CLEAN_TRUE,
                keys.state_turn(): turn.to_string(),
                keys.state_bytes(): bytes_raw.to_string(),
                keys.saved_at(): made_at.to_string(),
            },
        }
    })
}

pub fn idle_patch(keys: &PodKeys, released_at: Timestamp) -> Value {
    json!({ "metadata": { "annotations": { keys.idle_since(): released_at.to_string() } } })
}

pub fn seed_hash_patch(keys: &PodKeys, seed_hash: &str) -> Value {
    json!({ "metadata": { "annotations": { keys.seed_hash(): seed_hash } } })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColdPodMarks {
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
}

pub fn cold_pod_marks(
    keys: &PodKeys,
    pool_id: &PoolId,
    thread: ThreadRef,
    agent: AgentId,
    now: Timestamp,
) -> ColdPodMarks {
    ColdPodMarks {
        labels: BTreeMap::from([
            (keys.pool_id(), pool_id.to_string()),
            (keys.pool(), POOL_CLAIMED.to_owned()),
            (keys.thread(), thread.to_string()),
            (keys.agent(), agent.to_string()),
        ]),
        annotations: BTreeMap::from([
            (keys.clean(), CLEAN_FALSE.to_owned()),
            (keys.claimed_at(), now.to_string()),
        ]),
    }
}
