use br_agentic_contract::ThreadRef;
use uuid::Uuid;

use super::item::ItemKind;
use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReportKind {
    AttemptFailed,
    ItemRefused,
    ItemDroppedUnsent,
    ClosingFailed,
    RemoveFailed,
    Unsent,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct QueueReport {
    pub kind: ReportKind,
    pub thread: ThreadRef,
    pub message_id: Uuid,
    pub item_kind: ItemKind,
    pub item_id: Uuid,
    pub error: Option<Error>,
}

pub trait QueueObserver: Send + Sync {
    fn report(&self, report: QueueReport);
}

pub struct IgnoreReports;

impl QueueObserver for IgnoreReports {
    fn report(&self, _report: QueueReport) {}
}
