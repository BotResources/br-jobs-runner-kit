use std::time::Instant;

use br_agentic_contract::ThreadRef;
use uuid::Uuid;

use super::item::ItemKind;
use crate::error::Error;

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OldestItem {
    pub accepted_at: Instant,
    pub message_id: Uuid,
    pub kind: ItemKind,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FailedAttempt {
    pub item_id: Uuid,
    pub error: Error,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ThreadQueueState {
    pub thread: ThreadRef,
    pub waiting: usize,
    pub oldest: Option<OldestItem>,
    pub last_failure: Option<FailedAttempt>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct QueueState {
    pub threads: Vec<ThreadQueueState>,
}
