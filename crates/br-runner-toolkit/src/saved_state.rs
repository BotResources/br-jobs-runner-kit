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

#[cfg(test)]
mod tests {
    use br_graph_contract::State;
    use br_runner_jobs::test_double::ScriptedJobs;
    use br_runner_jobs::{AgentKey, RestoredAgent};

    use super::{decode, encode};
    use crate::unit_support::{Steps, book_with_agents, id, queue_of, runner_type, work};

    fn restored(node: &str, label: &str, n: u128, has_inbox: bool) -> RestoredAgent {
        RestoredAgent {
            key: AgentKey::new(node, label),
            agent_id: id(n),
            has_inbox,
        }
    }

    #[tokio::test]
    async fn given_a_state_and_two_agents_with_distinct_fields_when_it_is_saved_and_restored_then_every_field_of_every_agent_and_the_fields_of_the_state_come_back()
     {
        let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
        let work = work(&jobs, 1);
        let queue = queue_of(&work).await;
        let book = book_with_agents(
            &work,
            &queue,
            vec![
                restored("planner", "lead", 601, true),
                restored("writer", "side", 602, false),
            ],
        );
        let agents = book.agents();
        let state = State::create(Steps {
            steps: vec!["a".to_owned(), "b".to_owned()],
        })
        .expect("a state");
        let value = encode(&state, &agents).expect("encoded");
        let back = decode::<Steps>(value, None).expect("decoded");
        assert_eq!(back.fields.steps, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(back.agents.len(), 2);
        for agent in &agents {
            let found = back
                .agents
                .iter()
                .find(|restored| restored.key == agent.key)
                .expect("the agent comes back by its key");
            assert_eq!(found.agent_id, agent.agent_id);
            assert_eq!(found.has_inbox, agent.has_inbox);
        }
        let lead = back
            .agents
            .iter()
            .find(|agent| agent.key == AgentKey::new("planner", "lead"))
            .expect("lead");
        assert_eq!(lead.agent_id, id(601));
        assert!(lead.has_inbox);
        let side = back
            .agents
            .iter()
            .find(|agent| agent.key == AgentKey::new("writer", "side"))
            .expect("side");
        assert_eq!(side.agent_id, id(602));
        assert!(!side.has_inbox);
    }

    #[test]
    fn given_a_document_that_does_not_fit_when_it_is_restored_then_it_is_an_error() {
        assert!(decode::<Steps>(serde_json::json!("garbage"), None).is_err());
    }
}
