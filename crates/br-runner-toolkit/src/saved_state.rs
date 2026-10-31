use std::num::NonZeroUsize;

use br_graph_contract::{Fields, State};
use br_runner_jobs::{AgentKey, BookedAgent, RestoredAgent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct SavedAgent {
    node: String,
    label: String,
    agent_id: Uuid,
    has_inbox: bool,
}

#[derive(Serialize, Deserialize)]
struct Document {
    fields: Value,
    agents: Vec<SavedAgent>,
}

pub(crate) struct Restored<F> {
    pub fields: F,
    pub agents: Vec<RestoredAgent>,
}

pub(crate) fn encode<F: Fields>(state: &State<F>, agents: &[BookedAgent]) -> Result<Value, String> {
    let json = state.to_json().map_err(|e| e.to_string())?;
    let fields: Value = serde_json::from_slice(&json).map_err(|e| e.to_string())?;
    let document = Document {
        fields,
        agents: agents
            .iter()
            .map(|agent| SavedAgent {
                node: agent.key.node.clone(),
                label: agent.key.label.clone(),
                agent_id: agent.agent_id,
                has_inbox: agent.has_inbox,
            })
            .collect(),
    };
    serde_json::to_value(document).map_err(|e| e.to_string())
}

pub(crate) fn decode<F: Fields>(
    value: Value,
    limit: Option<NonZeroUsize>,
) -> Result<Restored<F>, String> {
    let document: Document = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&document.fields).map_err(|e| e.to_string())?;
    let state = match limit {
        Some(limit) => State::<F>::rebuild_with_limit(&bytes, limit),
        None => State::<F>::rebuild(&bytes),
    }
    .map_err(|e| e.to_string())?;
    Ok(Restored {
        fields: state.into_fields(),
        agents: document
            .agents
            .into_iter()
            .map(|agent| RestoredAgent {
                key: AgentKey::new(agent.node, agent.label),
                agent_id: agent.agent_id,
                has_inbox: agent.has_inbox,
            })
            .collect(),
    })
}
