use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use br_runner_jobs::{DeliveryQueue, QueueSettings, Removal, Work};
use futures::FutureExt;
use uuid::Uuid;

use super::Source;
use crate::observer::RunObserver;
use crate::record::RunRecord;
use crate::shared::Shared;

pub(super) async fn open_queue(
    shared: &Arc<Shared>,
    record: &Arc<RunRecord>,
    work: Work,
    source: Source,
) -> Option<(Arc<DeliveryQueue>, Work)> {
    let settings = QueueSettings {
        start_id: Uuid::now_v7(),
        outcome_wait: shared.settings.outcome_wait(),
        retry_wait: shared.settings.retry_wait(),
        observer: Arc::new(RunObserver {
            run_id: record.run_id,
            job_id: record.job_id,
            tap: shared.calls.tap().clone(),
            author: shared.author_observer.clone(),
        }),
    };
    let made = AssertUnwindSafe(work.delivery_queue(settings))
        .catch_unwind()
        .await;
    let (queue, started) = match made {
        Ok(Ok(pair)) => pair,
        other => {
            tracing::error!(run_id = %record.run_id, job_id = %record.job_id, "the delivery queue of the run cannot be made");
            drop(other);
            if source == Source::Trigger {
                shared.calls.give_back(work);
            }
            shared.forget_run(record.run_id);
            return None;
        }
    };
    let queue = Arc::new(queue);
    record.set_queue(Arc::clone(&queue));
    let ended = started.await;
    record.started_ended();
    match ended {
        Ok(()) => Some((queue, work)),
        Err(removal) => {
            if source == Source::Trigger && matches!(removal, Removal::Refused(_)) {
                shared.calls.acknowledge(Arc::new(work));
            }
            record.take_queue();
            shared.forget_run(record.run_id);
            None
        }
    }
}
