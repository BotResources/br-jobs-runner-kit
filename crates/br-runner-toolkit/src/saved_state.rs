use std::num::NonZeroUsize;

use br_agentic_contract::AgentLabel;
use br_graph_contract::{Fields, InvalidState, State};
use br_llm_messages::AgentId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agents::AgentKey;

const FORM: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedAgent {
    pub node: String,
    pub label: String,
    pub agent: String,
    pub inbox: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("saved state: {0}")]
pub struct SavedStateError(String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedState {
    #[serde(default)]
    form: u32,
    fields: Value,
    #[serde(default)]
    agents: Vec<SavedAgent>,
}

impl SavedState {
    pub fn from_state<F: Fields>(
        state: &State<F>,
        agents: Vec<SavedAgent>,
    ) -> Result<Self, SavedStateError> {
        let json = state
            .to_json()
            .map_err(|error| SavedStateError(error.to_string()))?;
        let fields =
            serde_json::from_slice(&json).map_err(|error| SavedStateError(error.to_string()))?;
        Ok(Self {
            form: FORM,
            fields,
            agents,
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, SavedStateError> {
        serde_json::to_vec(self).map_err(|error| SavedStateError(error.to_string()))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SavedStateError> {
        serde_json::from_slice(bytes).map_err(|error| SavedStateError(error.to_string()))
    }

    pub fn rebuild<F: Fields>(&self, limit: NonZeroUsize) -> Result<State<F>, InvalidState> {
        let json = serde_json::to_vec(&self.fields).unwrap_or_default();
        State::rebuild_with_limit(&json, limit)
    }

    pub fn agents(&self) -> &[SavedAgent] {
        &self.agents
    }

    pub(crate) fn keyed(&self) -> Result<Vec<(AgentKey, AgentId, bool)>, SavedStateError> {
        self.agents
            .iter()
            .map(|saved| {
                let label = AgentLabel::new(saved.label.clone())
                    .map_err(|error| SavedStateError(error.to_string()))?;
                let agent = AgentId::parse(&saved.agent)
                    .map_err(|error| SavedStateError(error.to_string()))?;
                Ok((AgentKey::new(saved.node.clone(), label), agent, saved.inbox))
            })
            .collect()
    }
}
