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

#[derive(Debug, Clone)]
pub struct WholeMessage {
    pub command_id: Uuid,
    pub thread: br_agentic_contract::ThreadRef,
    pub parent_message_id: Option<Uuid>,
    pub agent_id: AgentId,
    pub author_name: String,
    pub job_id: Option<Uuid>,
    pub acting_user_id: Option<Uuid>,
    pub impersonator_id: Option<Uuid>,
    pub turn: Turn,
    pub annotations: Vec<PlacedAnnotation>,
    pub wait: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct OpenStream {
    pub command_id: Uuid,
    pub thread: br_agentic_contract::ThreadRef,
    pub message_id: TurnId,
    pub parent_message_id: Option<Uuid>,
    pub agent_id: AgentId,
    pub author_name: String,
    pub job_id: Option<Uuid>,
    pub acting_user_id: Option<Uuid>,
    pub wait: Option<Duration>,
}
