mod agents;
mod inbox;
#[cfg(test)]
pub(crate) mod tests;

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use futures::FutureExt;
use uuid::Uuid;

use crate::arrival::Arrival;
use crate::caller::CallerHandle;
use crate::endwait::EndStore;
use crate::error::Error;
use crate::model::AgentDeclaration;
use crate::queue::{QueueWriter, Write};
use crate::work::WorkCore;
use agents::Agents;
use inbox::{Inbox, Wake};

pub(crate) use agents::{BookedAgentRef, RegistrationEnd};
pub(crate) use inbox::{EntryKind, ItemWatch, MailEntry};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentKey {
    pub node: String,
    pub label: String,
}

impl AgentKey {
    pub fn new(node: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            node: node.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredAgent {
    pub key: AgentKey,
    pub agent_id: Uuid,
    pub has_inbox: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BookedAgent {
    pub key: AgentKey,
    pub agent_id: Uuid,
    pub has_inbox: bool,
}

pub(crate) struct BookInner {
    core: Arc<WorkCore>,
    writer: QueueWriter,
    can_restart: bool,
    agents: Mutex<Agents>,
    inbox: Mutex<Inbox>,
    ends: Arc<EndStore>,
}

impl BookInner {
    fn agents(&self) -> MutexGuard<'_, Agents> {
        self.agents.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn inbox(&self) -> MutexGuard<'_, Inbox> {
        self.inbox.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn register(
        &self,
        agents: &mut Agents,
        key: AgentKey,
        agent_id: Uuid,
        has_inbox: bool,
    ) -> BookedAgentRef {
        let registration = self
            .writer
            .hand_over(Write::Registration {
                run_id: self.core.run_id,
                agent: AgentDeclaration {
                    agent_id,
                    has_inbox,
                    can_restart: self.can_restart,
                },
            })
            .shared();
        agents.add(key, agent_id, has_inbox, registration)
    }
}

pub struct AgentBook {
    inner: Arc<BookInner>,
}

fn check_restored(restored: &[RestoredAgent]) -> Result<(), Error> {
    let mut keys = HashSet::new();
    let mut ids = HashSet::new();
    for agent in restored {
        if !keys.insert(&agent.key) {
            return Err(Error::invalid("a restored agent key is given twice"));
        }
        if !ids.insert(agent.agent_id) {
            return Err(Error::invalid("a restored agent id is given twice"));
        }
    }
    Ok(())
}

impl AgentBook {
    pub(crate) fn make(
        core: Arc<WorkCore>,
        writer: QueueWriter,
        restored: Vec<RestoredAgent>,
        can_restart: bool,
    ) -> Result<Self, Error> {
        check_restored(&restored)?;
        WorkCore::claim_once(&core.book_made, "agent book")?;
        let inner = Arc::new(BookInner {
            core,
            writer,
            can_restart,
            agents: Mutex::new(Agents::default()),
            inbox: Mutex::new(Inbox::default()),
            ends: Arc::new(EndStore::new()),
        });
        {
            let mut agents = inner.agents();
            for agent in restored {
                inner.register(&mut agents, agent.key, agent.agent_id, agent.has_inbox);
            }
        }
        Ok(Self { inner })
    }

    pub fn agents(&self) -> Vec<BookedAgent> {
        self.inner.agents().listed()
    }

    pub fn caller_handle(&self) -> CallerHandle {
        CallerHandle::live(
            Arc::clone(&self.inner.core),
            self.inner.writer.clone(),
            Arc::downgrade(&self.inner.ends),
        )
    }

    pub fn arrival(&self, event: &Arrival) {
        match event {
            Arrival::Item(item) => self.inner.inbox().wake_address(item),
            Arrival::SyncEnd { child_job_id, end } => {
                self.inner.ends.record(*child_job_id, Ok(end.clone()));
            }
            Arrival::Unreadable {
                child_job_id: Some(child_job_id),
                error,
            } => self.inner.ends.record(*child_job_id, Err(error.clone())),
            Arrival::Unreadable {
                child_job_id: None, ..
            } => {}
        }
    }

    pub fn arrival_ended(&self, error: &Error) {
        if matches!(error, Error::Unreadable(_)) {
            return;
        }
        if matches!(error, Error::InfraMissing(_)) {
            self.inner.ends.infra_missing(error.clone());
        }
        self.inner.inbox().wake_all(&Wake::FeedEnded(error.clone()));
    }

    pub fn arrival_open_again(&self) {
        self.inner.ends.reopened();
    }

    pub(crate) fn agent_for(&self, key: &AgentKey, inbox_label: bool) -> BookedAgentRef {
        let mut agents = self.inner.agents();
        if let Some(found) = agents.find(key) {
            return found;
        }
        self.inner
            .register(&mut agents, key.clone(), Uuid::now_v7(), inbox_label)
    }

    pub(crate) fn holds_inbox(&self, agent_id: Uuid) -> bool {
        self.inner.agents().holds_inbox(agent_id)
    }

    pub(crate) fn registration_end(&self, agent_id: Uuid) -> Option<RegistrationEnd> {
        self.inner.agents().registration(agent_id)
    }

    pub(crate) fn record_listed(&self, agent_id: Uuid, entry_id: Uuid, entry: MailEntry) {
        self.inner.inbox().record_listed(agent_id, entry_id, entry);
    }

    pub(crate) fn listed(&self, agent_id: Uuid, entry_id: Uuid) -> Option<MailEntry> {
        self.inner.inbox().listed(agent_id, entry_id)
    }

    pub(crate) fn take_mail(&self, mail_id: Uuid) -> bool {
        self.inner.inbox().take_mail(mail_id)
    }

    pub(crate) fn mail_taken(&self, mail_id: Uuid) -> bool {
        self.inner.inbox().mail_taken(mail_id)
    }

    pub(crate) fn mark(&self, agent_id: Uuid, item_id: Uuid) {
        self.inner.inbox().mark(agent_id, item_id);
    }

    pub(crate) fn is_marked(&self, agent_id: Uuid, item_id: Uuid) -> bool {
        self.inner.inbox().is_marked(agent_id, item_id)
    }

    pub(crate) fn marked_items(&self, agent_id: Uuid) -> HashSet<Uuid> {
        self.inner.inbox().marked_items(agent_id)
    }

    pub(crate) fn subscribe_items(&self, address: Uuid) -> ItemWatch {
        self.inner.inbox().watch(address)
    }

    pub(crate) fn arrival_state(&self) -> Option<Error> {
        self.inner.ends.kept()
    }

    pub(crate) fn queue_writer(&self) -> QueueWriter {
        self.inner.writer.clone()
    }

    pub(crate) fn core(&self) -> &Arc<WorkCore> {
        &self.inner.core
    }
}

impl fmt::Debug for AgentBook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentBook")
            .field("agents", &self.agents())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for AgentBook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an agent book")
    }
}
