mod arrival;
mod closing;
mod finish;
mod prelude;

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use br_graph_contract::{CancelToken, Fields};
use br_graph_executor::{Outcome, RunConfig, RunFailure, start};
use br_runner_jobs::{DeliveryQueue, QueueSettings, Removal, Work};
use futures::FutureExt;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::exec_observer::ExecObserver;
use crate::failure::{Found, failed_of};
use crate::hooks;
use crate::observer::RunObserver;
use crate::plan::Plan;
use crate::record::{Phase, RunRecord};
use crate::shared::Shared;
use finish::{Ending, hand_over};
use prelude::{Early, prepare};

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

fn found_of(failure: RunFailure) -> Found {
    let (code, text, cause) = match failure {
        RunFailure::NodeFailed { reason, cause, .. } => ("node_failed", reason, cause),
        RunFailure::NodeRefusedInput { reason, .. } => ("node_refused_an_input", reason, None),
        RunFailure::NodeCouldNotStart { reason, cause, .. } => {
            ("node_could_not_start", reason, cause)
        }
        RunFailure::NodeVanished { reason, .. } => ("node_vanished", reason, None),
        RunFailure::UpdateRefused { reason, .. } => ("update_refused", reason, None),
        RunFailure::EdgeFailed { reason, cause, .. } => ("edge_failed", reason, cause),
        RunFailure::CheckpointNotBuilt { reason } => ("checkpoint_not_built", reason, None),
        RunFailure::SinkRefused { reason, .. } => ("sink_refused", reason, None),
        other => ("run_failed", other.to_string(), None),
    };
    let mut found = Found::permanent(code, text);
    found.cause = cause;
    found
}

fn ending_of<F: Fields>(ctx: &Ctx<F>, outcome: Outcome<F>) -> Ending<F> {
    match outcome {
        Outcome::RunEnded { state } => Ending::Ended(state),
        Outcome::RunFailed { failures, .. } => {
            let first = failures.into_iter().next().map_or_else(
                || Found::permanent("run_failed", "the executor reported no failure"),
                found_of,
            );
            Ending::Failed(failed_of(first, ctx.plan.mapping.as_deref()))
        }
        Outcome::RunCancelled { failures, .. } => {
            tracing::info!(
                run_id = %ctx.record.run_id,
                failures = failures.len(),
                "the run was cancelled"
            );
            Ending::Cancelled
        }
        _ => Ending::Failed(failed_of(
            Found::permanent(
                "run_failed",
                "the executor gave an outcome that is not known",
            ),
            ctx.plan.mapping.as_deref(),
        )),
    }
}

async fn body<F: Fields>(ctx: &Ctx<F>, token: CancelToken) -> Ending<F> {
    let view = token.view();
    let ready = match prepare(ctx, &view).await {
        Ok(ready) => ready,
        Err(Early::Cancelled) => return Ending::Cancelled,
        Err(Early::Failed(failed)) => return Ending::Failed(failed),
    };
    let mut config = RunConfig::new(token, ctx.shared.settings.cancel_grace());
    config.observer = Some(Arc::new(ExecObserver {
        shared: Arc::clone(&ctx.shared),
        record: Arc::clone(&ctx.record),
    }));
    ctx.record.set_executor_running(true);
    let outcome = start(
        Arc::clone(&ctx.plan.graph),
        ready.state,
        ready.nodes,
        config,
    )
    .await;
    ctx.record.set_executor_running(false);
    match outcome {
        Ok(outcome) => ending_of(ctx, outcome),
        Err(error) => Ending::Failed(failed_of(
            Found::permanent("start_refused", error.to_string()),
            ctx.plan.mapping.as_deref(),
        )),
    }
}

async fn open_queue(
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
            if source == Source::Trigger
                && let Err(error) = shared.calls.give_back(work).await
            {
                tracing::error!(%error, "the trigger could not be given back");
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
            tracing::error!(run_id = %record.run_id, job_id = %record.job_id, "the run is not executed, its started fact was removed");
            if source == Source::Trigger
                && matches!(removal, Removal::Refused(_))
                && let Err(error) = shared.calls.acknowledge(&work).await
            {
                tracing::error!(%error, "the trigger could not be acknowledged");
            }
            record.take_queue();
            shared.forget_run(record.run_id);
            None
        }
    }
}

pub(crate) async fn execute<F: Fields>(
    shared: &Arc<Shared>,
    plan: &Arc<Plan<F>>,
    work: Work,
    source: Source,
) {
    let (record, token) = shared.register_run(work.run_id(), work.job_id());
    let Some((queue, work)) = open_queue(shared, &record, work, source).await else {
        return;
    };
    let work = Arc::new(work);
    if source == Source::Trigger {
        let (shared, work) = (Arc::clone(shared), Arc::clone(&work));
        tokio::spawn(async move {
            if let Err(error) = shared.calls.acknowledge(&work).await {
                tracing::error!(%error, "the trigger could not be acknowledged");
            }
        });
    }
    let ctx = Ctx {
        shared: Arc::clone(shared),
        plan: Arc::clone(plan),
        record: Arc::clone(&record),
        queue: Arc::clone(&queue),
        work,
    };
    let ending = match AssertUnwindSafe(body(&ctx, token)).catch_unwind().await {
        Ok(ending) => ending,
        Err(payload) => {
            ctx.record.set_executor_running(false);
            Ending::Failed(failed_of(
                Found::permanent("panic", crate::panic::text_of(payload)),
                plan.mapping.as_deref(),
            ))
        }
    };
    let terminal = settle_and_hand_over(&ctx, ending).await;
    shared.instances.remove_run(record.run_id);
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
        .add(record.run_id, handle.abort_handle(), Arc::clone(&record));
    let _ = registered_sender.send(());
}

async fn settle_and_hand_over<F: Fields>(ctx: &Ctx<F>, ending: Ending<F>) -> finish::Terminal {
    ctx.record.set_phase(Phase::ExecutorReturned);
    hooks::call_all(&ctx.shared, &ctx.record);
    let waiting = ctx.record.outstanding_hooks() > 0 || !ctx.queue.state().waiting.is_empty();
    if waiting {
        tracing::warn!(
            run_id = %ctx.record.run_id,
            job_id = %ctx.record.job_id,
            hooks_running = ctx.record.outstanding_hooks(),
            writes_waiting = ctx.queue.state().waiting.len(),
            "the run waits to be settled"
        );
    }
    ctx.record.hooks_returned().await;
    ctx.queue.drained().await;
    if waiting {
        tracing::info!(run_id = %ctx.record.run_id, job_id = %ctx.record.job_id, "the run is settled");
    }
    let agents = ctx
        .record
        .book()
        .map(|book| book.agents())
        .unwrap_or_default();
    hand_over(ctx, &ctx.queue, &agents, ending)
}
