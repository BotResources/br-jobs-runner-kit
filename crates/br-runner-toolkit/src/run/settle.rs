use std::sync::Arc;

use br_graph_contract::Fields;
use br_runner_jobs::DeliveryQueue;

use super::Ctx;
use super::finish::{Ending, Terminal, hand_over};
use crate::record::{Phase, RunRecord};
use crate::shared::Shared;

pub(super) async fn settle_and_hand_over<F: Fields>(ctx: &Ctx<F>, ending: Ending<F>) -> Terminal {
    settle(&ctx.shared, &ctx.record, &ctx.queue).await;
    let agents = ctx
        .record
        .book()
        .map(|book| book.agents())
        .unwrap_or_default();
    hand_over(ctx, &ctx.queue, &agents, ending)
}

async fn settle(shared: &Arc<Shared>, record: &Arc<RunRecord>, queue: &DeliveryQueue) {
    if record.observations_expected() {
        record.observations_ended().await;
    }
    record.set_phase(Phase::ExecutorReturned);
    let mut drained = std::pin::pin!(queue.drained());
    let drained_early = futures::poll!(drained.as_mut());
    crate::hooks::call_all(shared, record);
    let waiting_writes = queue.state().waiting;
    let hooks_running = record.running_hooks();
    let waiting = !hooks_running.is_empty() || !waiting_writes.is_empty();
    if waiting {
        tracing::warn!(
            run_id = %record.run_id,
            job_id = %record.job_id,
            hooks_running = ?hooks_running,
            writes_waiting = ?waiting_writes,
            "the run waits to be settled"
        );
    }
    record.hooks_returned().await;
    if drained_early.is_pending() {
        drained.await;
    }
    if waiting {
        tracing::info!(run_id = %record.run_id, job_id = %record.job_id, "the run is settled");
    }
}

#[cfg(test)]
mod tests;
