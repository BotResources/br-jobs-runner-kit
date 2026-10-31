use std::fmt;
use std::sync::Arc;

use br_agentic_contract::{AgentLabel, PortError};
use br_llm_messages::AgentId;

use super::agent_port::AgentPortAdapter;
use super::gate::MarkGate;
use super::inbox_port::InboxPortAdapter;
use super::lookup::BookLookup;
use crate::book::{AgentBook, AgentKey};

pub(super) struct InboxSetup {
    pub label: AgentLabel,
    pub gate: MarkGate,
}

pub(super) struct Shared {
    pub node: String,
    pub lookup: Arc<dyn BookLookup>,
    pub inbox: Option<InboxSetup>,
}

impl Shared {
    pub(super) fn inbox_key(&self) -> Option<AgentKey> {
        self.inbox
            .as_ref()
            .map(|inbox| AgentKey::new(self.node.clone(), inbox.label.as_str()))
    }

    pub(super) fn agent_id(
        &self,
        book: &AgentBook,
        label: &AgentLabel,
    ) -> Result<AgentId, PortError> {
        let has_inbox = self
            .inbox
            .as_ref()
            .is_some_and(|inbox| inbox.label == *label);
        let key = AgentKey::new(self.node.clone(), label.as_str());
        let agent = book.agent_for(&key, has_inbox);
        AgentId::from_uuid(agent.agent_id).map_err(PortError::fault)
    }
}

#[derive(Clone)]
pub struct NodeEntry {
    shared: Arc<Shared>,
}

impl NodeEntry {
    pub fn new(node_name: impl Into<String>, lookup: Arc<dyn BookLookup>) -> Self {
        Self {
            shared: Arc::new(Shared {
                node: node_name.into(),
                lookup,
                inbox: None,
            }),
        }
    }

    pub fn with_inbox(
        node_name: impl Into<String>,
        lookup: Arc<dyn BookLookup>,
        inbox_label: AgentLabel,
        mark_gate: MarkGate,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                node: node_name.into(),
                lookup,
                inbox: Some(InboxSetup {
                    label: inbox_label,
                    gate: mark_gate,
                }),
            }),
        }
    }

    pub fn agent_port(&self) -> AgentPortAdapter {
        AgentPortAdapter::new(Arc::clone(&self.shared))
    }

    pub fn inbox_port(&self) -> Option<InboxPortAdapter> {
        self.shared
            .inbox
            .as_ref()
            .map(|_| InboxPortAdapter::new(Arc::clone(&self.shared)))
    }

    pub fn agent_id(&self, book: &AgentBook, label: &AgentLabel) -> Result<AgentId, PortError> {
        self.shared.agent_id(book, label)
    }
}

impl fmt::Debug for NodeEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeEntry")
            .field("node", &self.shared.node)
            .field("has_inbox", &self.shared.inbox.is_some())
            .finish_non_exhaustive()
    }
}
