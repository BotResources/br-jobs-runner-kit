use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NewMailInput {
    pub mail_id: Uuid,
    pub from_address: Uuid,
    pub author: String,
    pub source_conversation: Uuid,
    pub subject: String,
    pub body: String,
    pub to: Vec<Uuid>,
    pub cc: Vec<Uuid>,
    pub attachments: Vec<NewMailAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMailAttachment {
    pub attachment_id: Uuid,
    pub name: String,
    pub content_type: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ListedAgent {
    pub agent_id: Uuid,
    pub run_id: Uuid,
    pub runner_type: String,
    pub running: bool,
    pub has_inbox: bool,
    pub mail_address: Option<Uuid>,
    pub restartable_now: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDeclaration {
    pub agent_id: Uuid,
    pub has_inbox: bool,
    pub can_restart: bool,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SavedState {
    pub state: Value,
    pub agent_ids: Vec<Uuid>,
    pub mail_addresses: Vec<Uuid>,
}
