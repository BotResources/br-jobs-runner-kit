mod arrival;
mod body;
mod closing;
mod finish;
mod prelude;
mod queue;
mod settle;

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use br_graph_contract::Fields;
use br_runner_jobs::{DeliveryQueue, Work};
use futures::FutureExt;
use tokio::sync::oneshot;

use crate::failure::{Found, failed_of};
use crate::plan::Plan;
use crate::record::{Phase, RunRecord};
use crate::shared::Shared;
use crate::slot::settle_known;
use finish::Terminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Trigger,
    Claim,
}

pub(crate) struct Ctx<F: Fields> {
    pub shared: Arc<Shared>,
    pub plan: Arc<Plan<F>>,
    pub record: Arc<RunRecord>,
    pub queue: Arc<DeliveryQueue>,
    pub work: Arc<Work>,
}

pub(crate) async fn execute<F: Fields>(
    shared: &Arc<Shared>,
    plan: &Arc<Plan<F>>,
    work: Work,
    source: Source,
) {
    let (record, token) = match shared.register_run(work.run_id(), work.job_id()) {
        Ok(registered) => registered,
        Err(known) => {
            match source {
                Source::Trigger => settle_known(shared, &known, work),
                Source::Claim => tracing::error!(
                    run_id = %known.run_id,
                    "jobs gave a run that is already in flight on this pod, it is dropped"
                ),
            }
            return;
        }
    };
    let Some((queue, work)) = queue::open_queue(shared, &record, work, source).await else {
        return;
    };
    let work = Arc::new(work);
    if source == Source::Trigger {
        shared.calls.acknowledge(Arc::clone(&work));
    }
    let ctx = Ctx {
        shared: Arc::clone(shared),
        plan: Arc::clone(plan),
        record: Arc::clone(&record),
        queue: Arc::clone(&queue),
        work,
    };
    let run = AssertUnwindSafe(async {
        let ending = body::body(&ctx, token).await;
        settle::settle_and_hand_over(&ctx, ending).await
    })
    .catch_unwind()
    .await;
    let terminal = match run {
        Ok(terminal) => terminal,
        Err(payload) => {
            record.set_executor_running(false);
            let failed = failed_of(
                Found::permanent("panic", crate::panic::text_of(payload)),
                plan.mapping.as_deref(),
            );
            Terminal::Failed(queue.failed(failed))
        }
    };
    shared.instances.remove_run(&record, Phase::Dropped);
    record.stop_arrival();
    drop(record.take_book());
    let (registered_sender, registered) = oneshot::channel();
    let handle = tokio::spawn(closing::run(
        Arc::clone(shared),
        Arc::clone(&record),
        queue,
        terminal,
        plan.mapping.clone(),
        registered,
    ));
    shared
        .closings
        .add(record.run_id, handle, Arc::clone(&record));
    let _ = registered_sender.send(());
}
