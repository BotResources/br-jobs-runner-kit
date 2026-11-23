use std::fmt;
use std::sync::Arc;

use br_agentic_contract::{AgentLabel, PortError};
use br_llm_messages::AgentId;
use br_runner_jobs_client::{AgentBook, AgentKey, BookLookup};

use super::agent_port::AgentPortAdapter;
use super::gate::MarkGate;
use super::incoming_port::IncomingPortAdapter;
use super::mail_port::MailPortAdapter;

pub(super) struct InboxSetup {
    pub label: AgentLabel,
    pub gate: Option<MarkGate>,
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
                    gate: Some(mark_gate),
                }),
            }),
        }
    }

    pub fn with_mail(
        node_name: impl Into<String>,
        lookup: Arc<dyn BookLookup>,
        inbox_label: AgentLabel,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                node: node_name.into(),
                lookup,
                inbox: Some(InboxSetup {
                    label: inbox_label,
                    gate: None,
                }),
            }),
        }
    }

    pub fn agent_port(&self) -> AgentPortAdapter {
        AgentPortAdapter::new(Arc::clone(&self.shared))
    }

    pub fn incoming_port(&self) -> Option<IncomingPortAdapter> {
        self.shared
            .inbox
            .as_ref()
            .and_then(|inbox| inbox.gate.clone())
            .map(|gate| IncomingPortAdapter::new(Arc::clone(&self.shared), gate))
    }

    pub fn mail_port(&self) -> Option<MailPortAdapter> {
        self.shared
            .inbox
            .as_ref()
            .filter(|inbox| inbox.gate.is_none())
            .map(|_| MailPortAdapter::new(Arc::clone(&self.shared)))
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
            .field(
                "has_gate",
                &self
                    .shared
                    .inbox
                    .as_ref()
                    .is_some_and(|inbox| inbox.gate.is_some()),
            )
            .finish_non_exhaustive()
    }
}
