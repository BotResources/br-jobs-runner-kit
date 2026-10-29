use std::sync::Arc;

use br_agentic_contract::AgentLabel;
use br_llm_clients_core::BoxFuture;
use br_llm_messages::AgentId;
use futures::FutureExt;
use futures::future::Shared;
use uuid::Uuid;

use crate::error::{PortError, refusal};
use crate::runs::{AgentRecord, Runs};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentKey {
    pub node: String,
    pub label: AgentLabel,
}

impl AgentKey {
    pub fn new(node: impl Into<String>, label: AgentLabel) -> Self {
        Self {
            node: node.into(),
            label,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentRegistration {
    pub agent: AgentId,
    pub has_inbox: bool,
    pub can_restart: bool,
}

pub type AgentRegistrar =
    Arc<dyn Fn(AgentRegistration) -> BoxFuture<'static, Result<(), PortError>> + Send + Sync>;

type Pending = Shared<BoxFuture<'static, Result<AgentId, PortError>>>;

pub(crate) enum KeySlot {
    Pending(Pending),
    Known(AgentId),
}

pub(crate) struct Declared {
    pub has_inbox: bool,
    pub can_restart: bool,
}

impl Runs {
    pub(crate) async fn identity(
        &self,
        run: Uuid,
        key: AgentKey,
        declared: Declared,
        registrar: AgentRegistrar,
    ) -> Result<AgentId, PortError> {
        let fresh = AgentId::from_uuid(Uuid::now_v7()).map_err(refusal)?;
        let pending = self.with(|state| match state.keys.get(&(run, key.clone())) {
            Some(KeySlot::Known(agent)) => Err(*agent),
            Some(KeySlot::Pending(pending)) => Ok(pending.clone()),
            None => {
                let agent = fresh;
                let registration = AgentRegistration {
                    agent,
                    has_inbox: declared.has_inbox,
                    can_restart: declared.has_inbox && declared.can_restart,
                };
                let pending = self.registering(run, key.clone(), registration, registrar);
                state
                    .keys
                    .insert((run, key), KeySlot::Pending(pending.clone()));
                Ok(pending)
            }
        });
        match pending {
            Err(agent) => Ok(agent),
            Ok(pending) => pending.await,
        }
    }

    fn registering(
        &self,
        run: Uuid,
        key: AgentKey,
        registration: AgentRegistration,
        registrar: AgentRegistrar,
    ) -> Pending {
        let runs = self.clone();
        let registered: BoxFuture<'static, Result<AgentId, PortError>> = Box::pin(async move {
            let outcome = registrar(registration).await;
            runs.with(|state| {
                if state.runs.get(&run) != Some(&false) {
                    return;
                }
                match &outcome {
                    Ok(()) => {
                        state
                            .keys
                            .insert((run, key.clone()), KeySlot::Known(registration.agent));
                        state.agents.insert(
                            registration.agent,
                            AgentRecord {
                                run,
                                key,
                                has_inbox: registration.has_inbox,
                            },
                        );
                    }
                    Err(_) => {
                        state.keys.remove(&(run, key));
                    }
                }
            });
            outcome.map(|()| registration.agent)
        });
        registered.shared()
    }

    pub(crate) fn preload(&self, run: Uuid, key: AgentKey, agent: AgentId, has_inbox: bool) {
        self.with(|state| {
            state.keys.insert((run, key.clone()), KeySlot::Known(agent));
            state.agents.insert(
                agent,
                AgentRecord {
                    run,
                    key,
                    has_inbox,
                },
            );
        });
    }

    pub(crate) fn agents_of(&self, run: Uuid) -> Vec<(AgentKey, AgentId, bool)> {
        self.with(|state| {
            state
                .agents
                .iter()
                .filter(|(_, record)| record.run == run)
                .map(|(agent, record)| (record.key.clone(), *agent, record.has_inbox))
                .collect()
        })
    }
}
