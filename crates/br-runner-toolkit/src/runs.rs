use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use br_graph_contract::InstanceId;
use br_graph_executor::{Observation, ObservedEvent, Observer};
use br_llm_messages::AgentId;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::agents::{AgentKey, KeySlot};
use crate::error::{PortError, PortErrorCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstanceRef {
    pub run: Uuid,
    pub node: String,
}

pub(crate) struct AgentRecord {
    pub run: Uuid,
    pub key: AgentKey,
    pub has_inbox: bool,
}

struct InstanceRecord {
    at: InstanceRef,
    live: bool,
}

#[derive(Default)]
pub(crate) struct State {
    pub runs: HashMap<Uuid, bool>,
    instances: HashMap<InstanceId, InstanceRecord>,
    pub agents: HashMap<AgentId, AgentRecord>,
    pub keys: HashMap<(Uuid, AgentKey), KeySlot>,
}

#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    changed: Notify,
}

#[derive(Clone, Default)]
pub(crate) struct Runs {
    inner: Arc<Inner>,
}

impl Runs {
    pub(crate) fn with<R>(&self, action: impl FnOnce(&mut State) -> R) -> R {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        action(&mut state)
    }

    pub(crate) fn begin(&self, run: Uuid) {
        self.with(|state| state.runs.insert(run, false));
    }

    pub(crate) fn end(&self, run: Uuid) {
        self.with(|state| {
            state.runs.insert(run, true);
            state.instances.retain(|_, record| record.at.run != run);
            state.agents.retain(|_, record| record.run != run);
            state.keys.retain(|(owner, _), _| *owner != run);
        });
        self.inner.changed.notify_waiters();
    }

    pub(crate) fn is_in_flight(&self, run: Uuid) -> bool {
        self.with(|state| state.runs.get(&run) == Some(&false))
    }

    pub(crate) fn observer(&self, run: Uuid, on_end: OnEnd) -> Arc<dyn Observer> {
        Arc::new(RunObserver {
            run,
            runs: self.clone(),
            on_end,
        })
    }

    pub(crate) async fn instance(&self, instance: InstanceId) -> Result<InstanceRef, PortError> {
        loop {
            let changed = self.inner.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(found) = self.with(|state| {
                state
                    .instances
                    .get(&instance)
                    .map(|record| record.at.clone())
            }) {
                return Ok(found);
            }
            changed.await;
        }
    }

    pub(crate) fn has_live_instance(&self, run: Uuid, node: &str) -> bool {
        self.with(|state| {
            state
                .instances
                .values()
                .any(|record| record.live && record.at.run == run && record.at.node == node)
        })
    }

    pub(crate) fn agent_in_flight(&self, agent: AgentId) -> Result<(Uuid, AgentKey), PortError> {
        self.with(|state| {
            let record = state.agents.get(&agent).ok_or_else(|| {
                PortError::new(
                    PortErrorCode::UnknownAgent,
                    format!("the agent {} belongs to no run", agent.as_uuid()),
                )
            })?;
            if state.runs.get(&record.run) != Some(&false) {
                return Err(PortError::new(
                    PortErrorCode::RunEnded,
                    "the run of the agent has ended",
                ));
            }
            Ok((record.run, record.key.clone()))
        })
    }

    pub(crate) fn live_agent(&self, agent: AgentId) -> Result<(Uuid, AgentKey), PortError> {
        let (run, key) = self.agent_in_flight(agent)?;
        if !self.has_live_instance(run, &key.node) {
            return Err(PortError::new(
                PortErrorCode::NoLiveInstance,
                format!("the node {} has no live instance", key.node),
            ));
        }
        Ok((run, key))
    }

    fn instance_created(&self, run: Uuid, instance: InstanceId, node: String) {
        self.with(|state| {
            if state.runs.get(&run) == Some(&false) {
                state.instances.insert(
                    instance,
                    InstanceRecord {
                        at: InstanceRef { run, node },
                        live: true,
                    },
                );
            }
        });
        self.inner.changed.notify_waiters();
    }

    fn instance_ended(&self, instance: InstanceId) -> Option<InstanceRef> {
        let ended = self.with(|state| {
            state.instances.get_mut(&instance).map(|record| {
                record.live = false;
                record.at.clone()
            })
        });
        self.inner.changed.notify_waiters();
        ended
    }
}

pub(crate) type OnEnd = Arc<dyn Fn(&InstanceRef) + Send + Sync>;

struct RunObserver {
    run: Uuid,
    runs: Runs,
    on_end: OnEnd,
}

impl Observer for RunObserver {
    fn observe(&self, observation: Observation) {
        match observation {
            Observation::InstanceCreated { instance, node, .. } => {
                self.runs.instance_created(self.run, instance, node);
            }
            Observation::EventHandled {
                instance, event, ..
            } if !matches!(event, ObservedEvent::Refused) => {
                if let Some(ended) = self.runs.instance_ended(instance) {
                    (self.on_end)(&ended);
                }
            }
            _ => {}
        }
    }
}
