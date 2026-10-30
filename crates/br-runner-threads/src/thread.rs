use std::collections::BTreeMap;
use std::time::Duration;

use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadOrigin {
    pub parent_thread: br_agentic_contract::ThreadRef,
    pub origin_block_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct NewThread {
    pub command_id: Uuid,
    pub thread: br_agentic_contract::ThreadRef,
    pub owner_id: Option<Uuid>,
    pub member_ids: Vec<Uuid>,
    pub title: Option<String>,
    pub resources: BTreeMap<String, Vec<String>>,
    pub origin: Option<ThreadOrigin>,
    pub acting_user_id: Option<Uuid>,
    pub wait: Option<Duration>,
}
