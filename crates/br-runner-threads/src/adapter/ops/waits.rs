use std::sync::Arc;

use br_agentic_contract::ThreadRef;
use br_llm_messages::{AgentId, EntryId};

use crate::adapter::inner::{Inner, Slot, locked};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryOutcome {
    Stored,
    Dropped,
}

impl Inner {
    pub(crate) async fn drained(&self, thread: &ThreadRef) {
        let slot = locked(&self.slots).get(thread).cloned();
        if let Some(slot) = slot {
            let target = locked(&slot.queue).last_no;
            wait_past(&slot, target).await;
        }
    }

    pub(crate) async fn entry_stored(&self, thread: &ThreadRef, entry: EntryId) -> EntryOutcome {
        let slot = locked(&self.slots).get(thread).cloned();
        let Some(slot) = slot else {
            return EntryOutcome::Stored;
        };
        let id = entry.as_uuid();
        loop {
            let mut changes = slot.changed.subscribe();
            let holds = {
                let queue = locked(&slot.queue);
                if queue.dropped.contains(&id) {
                    return EntryOutcome::Dropped;
                }
                queue
                    .items
                    .iter()
                    .any(|queued| queued.item.entries.contains(&id))
            };
            if !holds {
                return EntryOutcome::Stored;
            }
            if changes.changed().await.is_err() {
                return EntryOutcome::Stored;
            }
        }
    }

    pub(crate) async fn instance_ended(self: &Arc<Self>, agent: AgentId) {
        let id = agent.as_uuid();
        let mut waits = Vec::new();
        let mut involved = Vec::new();
        for slot in self.slots() {
            let holds_item = locked(&slot.queue)
                .items
                .iter()
                .any(|queued| queued.item.agent == id);
            let holds_state = {
                let mem = slot.mem.lock().await;
                mem.turns.values().any(|state| state.agent == agent)
                    || mem.responses.values().any(|open| open.agent == agent)
            };
            if holds_item || holds_state {
                waits.push((Arc::clone(&slot), locked(&slot.queue).last_no));
                involved.push(slot);
            }
        }
        for (slot, target) in waits {
            wait_past(&slot, target).await;
        }
        for slot in involved {
            let ended = {
                let mut mem = slot.mem.lock().await;
                mem.turns.retain(|_, state| state.agent != agent);
                let gone: Vec<EntryId> = mem
                    .responses
                    .iter()
                    .filter(|(_, open)| open.agent == agent)
                    .map(|(entry, _)| *entry)
                    .collect();
                gone.into_iter()
                    .filter_map(|entry| mem.responses.remove(&entry))
                    .collect::<Vec<_>>()
            };
            for open in ended {
                open.control.end().await;
            }
        }
    }
}

async fn wait_past(slot: &Slot, target: u64) {
    loop {
        let mut changes = slot.changed.subscribe();
        let done = locked(&slot.queue)
            .items
            .front()
            .is_none_or(|head| head.no > target);
        if done || changes.changed().await.is_err() {
            return;
        }
    }
}
