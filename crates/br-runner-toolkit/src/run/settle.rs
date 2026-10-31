use br_graph_contract::Fields;

use super::Ctx;
use super::finish::{Ending, Terminal, hand_over};

pub(super) async fn settle_and_hand_over<F: Fields>(ctx: &Ctx<F>, ending: Ending<F>) -> Terminal {
    ctx.record.set_phase(crate::record::Phase::ExecutorReturned);
    let mut drained = std::pin::pin!(ctx.queue.drained());
    let drained_early = futures::poll!(drained.as_mut());
    crate::hooks::call_all(&ctx.shared, &ctx.record);
    let waiting_writes = ctx.queue.state().waiting.len();
    let hooks_running = ctx.record.running_hooks();
    let waiting = !hooks_running.is_empty() || waiting_writes > 0;
    if waiting {
        tracing::warn!(
            run_id = %ctx.record.run_id,
            job_id = %ctx.record.job_id,
            hooks_running = ?hooks_running,
            writes_waiting = waiting_writes,
            "the run waits to be settled"
        );
    }
    ctx.record.hooks_returned().await;
    if drained_early.is_pending() {
        drained.await;
    }
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
