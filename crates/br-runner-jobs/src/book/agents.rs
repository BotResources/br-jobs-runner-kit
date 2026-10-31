use std::collections::HashMap;

use futures::future::Shared;
use uuid::Uuid;

use super::{AgentKey, BookedAgent};
use crate::queue::WriteEnd;

pub(crate) type RegistrationEnd = Shared<WriteEnd>;

#[derive(Clone)]
pub(crate) struct BookedAgentRef {
    pub agent_id: Uuid,
    pub has_inbox: bool,
    pub created: bool,
    pub registration: RegistrationEnd,
}

#[derive(Clone)]
struct Slot {
    agent_id: Uuid,
    has_inbox: bool,
    registration: RegistrationEnd,
}

impl Slot {
    fn reference(&self, created: bool) -> BookedAgentRef {
        BookedAgentRef {
            agent_id: self.agent_id,
            has_inbox: self.has_inbox,
            created,
            registration: self.registration.clone(),
        }
    }
}

#[derive(Default)]
pub(super) struct Agents {
    order: Vec<AgentKey>,
    by_key: HashMap<AgentKey, Slot>,
    by_id: HashMap<Uuid, AgentKey>,
}

impl Agents {
    pub(super) fn find(&self, key: &AgentKey) -> Option<BookedAgentRef> {
        self.by_key.get(key).map(|slot| slot.reference(false))
    }

    pub(super) fn add(
        &mut self,
        key: AgentKey,
        agent_id: Uuid,
        has_inbox: bool,
        registration: RegistrationEnd,
    ) -> BookedAgentRef {
        let slot = Slot {
            agent_id,
            has_inbox,
            registration,
        };
        let reference = slot.reference(true);
        self.order.push(key.clone());
        self.by_id.insert(agent_id, key.clone());
        self.by_key.insert(key, slot);
        reference
    }

    fn slot_of(&self, agent_id: Uuid) -> Option<&Slot> {
        self.by_id
            .get(&agent_id)
            .and_then(|key| self.by_key.get(key))
    }

    pub(super) fn holds_inbox(&self, agent_id: Uuid) -> bool {
        self.slot_of(agent_id).is_some_and(|slot| slot.has_inbox)
    }

    pub(super) fn registration(&self, agent_id: Uuid) -> Option<RegistrationEnd> {
        self.slot_of(agent_id).map(|slot| slot.registration.clone())
    }

    pub(super) fn listed(&self) -> Vec<BookedAgent> {
        self.order
            .iter()
            .filter_map(|key| {
                self.by_key.get(key).map(|slot| BookedAgent {
                    key: key.clone(),
                    agent_id: slot.agent_id,
                    has_inbox: slot.has_inbox,
                })
            })
            .collect()
    }
}
