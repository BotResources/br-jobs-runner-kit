use std::time::Duration;

use br_llm_messages::{AgentId, EntryId, Turn, TurnId};
use uuid::Uuid;

use crate::annotation::Annotation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageStatus {
    Streaming,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlacedAnnotation {
    pub after_entry: EntryId,
    pub annotation: Annotation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Confirmed {
    pub command_id: Uuid,
    pub status: Option<MessageStatus>,
}

#[derive(Debug, Clone)]
pub struct WholeMessage {
    pub command_id: Uuid,
    pub thread_id: Uuid,
    pub parent_message_id: Option<Uuid>,
    pub agent_id: AgentId,
    pub author_name: String,
    pub job_id: Option<Uuid>,
    pub acting_user_id: Option<Uuid>,
    pub impersonator_id: Option<Uuid>,
    pub turn: Turn,
    pub annotations: Vec<PlacedAnnotation>,
    pub wait: Duration,
}

#[derive(Debug, Clone)]
pub struct OpenStream {
    pub command_id: Uuid,
    pub thread_id: Uuid,
    pub message_id: TurnId,
    pub parent_message_id: Option<Uuid>,
    pub agent_id: AgentId,
    pub author_name: String,
    pub job_id: Option<Uuid>,
    pub acting_user_id: Option<Uuid>,
    pub wait: Duration,
}
