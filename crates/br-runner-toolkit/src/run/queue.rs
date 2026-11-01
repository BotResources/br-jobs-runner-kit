use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use br_runner_jobs::{DeliveryQueue, QueueSettings, Removal, Work};
use futures::FutureExt;
use uuid::Uuid;

use super::Source;
use crate::observer::RunObserver;
use crate::record::RunRecord;
use crate::shared::Shared;

fn unmade(
    shared: &Arc<Shared>,
    record: &Arc<RunRecord>,
    work: Work,
    source: Source,
    cause: String,
) {
    tracing::error!(
        run_id = %record.run_id,
        job_id = %record.job_id,
        cause = %cause,
        "the delivery queue of the run cannot be made"
    );
    if source == Source::Trigger {
        shared.calls.give_back(work);
    }
    shared.forget_run(record.run_id);
}

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
        Ok(Err(error)) => {
            unmade(shared, record, work, source, error.to_string());
            return None;
        }
        Err(payload) => {
            unmade(shared, record, work, source, crate::panic::text_of(payload));
            return None;
        }
    };
    let queue = Arc::new(queue);
    record.set_queue(Arc::clone(&queue));
    match started.await {
        Ok(()) => {
            record.started_ended();
            Some((queue, work))
        }
        Err(removal) => {
            if matches!(removal, Removal::Refused(_)) {
                record.started_ended();
                if source == Source::Trigger {
                    shared.calls.acknowledge(Arc::new(work));
                }
            }
            record.take_queue();
            shared.forget_run(record.run_id);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use br_runner_jobs::test_double::ScriptedJobs;
    use tracing::Level;

    use super::{Source, open_queue};
    use crate::instances::Instances;
    use crate::unit_support::logs::Logs;
    use crate::unit_support::{queue_of, record, shared, work};

    #[tokio::test]
    async fn given_a_work_whose_queue_was_made_already_when_the_queue_is_opened_then_the_error_log_names_the_cause()
     {
        let (logs, _guard) = Logs::capture();
        let jobs = ScriptedJobs::new("analyst", "i1");
        let instances = Arc::new(Instances::default());
        let shared = shared(&jobs, None, &instances);
        let work = work(&jobs, 1);
        let _first = queue_of(&work).await;
        let record = record(&work);
        let opened = open_queue(&shared, &record, work, Source::Claim).await;
        assert!(opened.is_none());
        assert_eq!(
            logs.containing(Level::ERROR, "of this Work was made already"),
            1
        );
        assert_eq!(
            logs.containing(Level::ERROR, "the delivery queue of the run cannot be made"),
            1
        );
    }
}
