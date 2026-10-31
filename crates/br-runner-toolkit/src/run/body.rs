use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use br_graph_contract::{CancelToken, Fields};
use br_graph_executor::{Outcome, RunConfig, RunFailure, start};
use futures::FutureExt;

use super::Ctx;
use super::finish::Ending;
use super::prelude::{Early, prepare};
use crate::exec_observer::ExecObserver;
use crate::failure::{Found, failed_of};

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
                job_id = %ctx.record.job_id,
                failures = ?failures,
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

async fn execute_graph<F: Fields>(ctx: &Ctx<F>, token: CancelToken) -> Ending<F> {
    let view = token.view();
    let ready = match prepare(ctx, &view).await {
        Ok(ready) => ready,
        Err(Early::Cancelled) => {
            tracing::info!(
                run_id = %ctx.record.run_id,
                job_id = %ctx.record.job_id,
                "the run was cancelled before its graph started"
            );
            return Ending::Cancelled;
        }
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

pub(super) async fn body<F: Fields>(ctx: &Ctx<F>, token: CancelToken) -> Ending<F> {
    match AssertUnwindSafe(execute_graph(ctx, token))
        .catch_unwind()
        .await
    {
        Ok(ending) => ending,
        Err(payload) => {
            ctx.record.set_executor_running(false);
            Ending::Failed(failed_of(
                Found::permanent("panic", crate::panic::text_of(payload)),
                ctx.plan.mapping.as_deref(),
            ))
        }
    }
}
