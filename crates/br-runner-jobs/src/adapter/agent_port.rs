use std::sync::Arc;

use br_agentic_contract::{AgentLabel, AgentPort, Fields, InstanceId, PortError, PortFuture};
use br_llm_messages::AgentId;

use super::node_entry::Shared;

#[derive(Clone)]
pub struct AgentPortAdapter {
    shared: Arc<Shared>,
}

impl AgentPortAdapter {
    pub(super) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

impl<F: Fields> AgentPort<F> for AgentPortAdapter {
    fn agent<'a>(
        &'a self,
        _snapshot: &'a F,
        instance: InstanceId,
        label: &'a AgentLabel,
    ) -> PortFuture<'a, Result<AgentId, PortError>> {
        Box::pin(async move {
            let book = self.shared.lookup.book(instance).await?;
            self.shared.agent_id(&book, label)
        })
    }
}

impl std::fmt::Debug for AgentPortAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentPortAdapter").finish_non_exhaustive()
    }
}
