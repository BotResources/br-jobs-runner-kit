use std::sync::{Arc, Mutex, PoisonError};

use br_runner_jobs::{LogLevel, PresenceStatus, QueueReport};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum HostEvent {
    Queue(QueueReport),
    TriggerAcknowledged,
    TriggerGivenBack,
    Claim {
        claim_id: Uuid,
    },
    PresencePublished {
        runner_version: String,
        status: PresenceStatus,
        capacity: u32,
    },
    PresenceDeleted,
    RunLog {
        level: LogLevel,
        message: String,
    },
}

#[derive(Default)]
pub(crate) struct Tap {
    events: Mutex<Vec<HostEvent>>,
}

impl Tap {
    pub(crate) fn record(&self, event: HostEvent) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
    }

    #[cfg(feature = "test-host")]
    pub(crate) fn events(&self) -> Vec<HostEvent> {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

pub(crate) fn tell(tap: &Option<Arc<Tap>>, event: impl FnOnce() -> HostEvent) {
    if let Some(tap) = tap {
        tap.record(event());
    }
}
