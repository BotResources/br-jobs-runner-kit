use std::sync::Arc;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::error::Error;
use crate::queue::write::{Removal, WriteName};

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum QueueReport {
    Retrying { write: WriteName, error: Error },
    Removed { write: WriteName, removal: Removal },
    Delivered { write: WriteName },
}

pub trait QueueObserver: Send + Sync {
    fn report(&self, report: &QueueReport);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct IgnoreReports;

impl QueueObserver for IgnoreReports {
    fn report(&self, _report: &QueueReport) {}
}

#[derive(Clone)]
pub struct QueueSettings {
    pub start_id: Uuid,
    pub outcome_wait: Duration,
    pub retry_wait: Duration,
    pub observer: Arc<dyn QueueObserver>,
}

impl std::fmt::Debug for QueueSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueueSettings")
            .field("start_id", &self.start_id)
            .field("outcome_wait", &self.outcome_wait)
            .field("retry_wait", &self.retry_wait)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LastFailure {
    pub write: WriteName,
    pub error: Error,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct QueueState {
    pub waiting: Vec<WriteName>,
    pub oldest_handed_over: Option<Instant>,
    pub last_failure: Option<LastFailure>,
}
