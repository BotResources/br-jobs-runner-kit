use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use br_runner_jobs::{QueueObserver, QueueReport};
use uuid::Uuid;

use crate::tap::{HostEvent, Tap};

pub(crate) struct RunObserver {
    pub run_id: Uuid,
    pub job_id: Uuid,
    pub tap: Option<Arc<Tap>>,
    pub author: Option<Arc<dyn QueueObserver>>,
}

impl QueueObserver for RunObserver {
    fn report(&self, report: &QueueReport) {
        match report {
            QueueReport::Retrying { write, error } => tracing::warn!(
                run_id = %self.run_id,
                job_id = %self.job_id,
                write = ?write,
                %error,
                "a write of the run failed and is sent again"
            ),
            QueueReport::Removed { write, removal } => tracing::error!(
                run_id = %self.run_id,
                job_id = %self.job_id,
                write = ?write,
                removal = ?removal,
                "a write of the run was removed"
            ),
            _ => {}
        }
        if let Some(tap) = &self.tap
            && matches!(
                report,
                QueueReport::Removed { .. } | QueueReport::Delivered { .. }
            )
        {
            tap.record(HostEvent::Queue(report.clone()));
        }
        if let Some(author) = &self.author
            && catch_unwind(AssertUnwindSafe(|| author.report(report))).is_err()
        {
            tracing::error!(
                run_id = %self.run_id,
                "the observer of the runner author panicked"
            );
        }
    }
}
