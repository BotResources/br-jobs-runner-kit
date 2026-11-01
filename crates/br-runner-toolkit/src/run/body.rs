use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use br_graph_contract::{CancelToken, Fields};
use br_graph_executor::{Outcome, RunConfig, RunFailure, StartError, start};
use futures::FutureExt;

use super::Ctx;
use super::finish::Ending;
use super::prelude::{Early, prepare};
use crate::exec_observer::ExecObserver;
use crate::failure::{FailureMapping, Found, failed_of};
use crate::record::RunRecord;

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

fn first_found(failures: Vec<RunFailure>) -> Found {
    failures.into_iter().next().map_or_else(
        || Found::permanent("run_failed", "the executor reported no failure"),
        found_of,
    )
}

fn ending_of<F: Fields>(
    record: &RunRecord,
    mapping: Option<&dyn FailureMapping>,
    outcome: Outcome<F>,
) -> Ending<F> {
    match outcome {
        Outcome::RunEnded { state } => Ending::Ended(state),
        Outcome::RunFailed { failures, .. } => {
            Ending::Failed(failed_of(first_found(failures), mapping))
        }
        Outcome::RunCancelled { failures, .. } => {
            tracing::info!(
                run_id = %record.run_id,
                job_id = %record.job_id,
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
            mapping,
        )),
    }
}

fn after_start<F: Fields>(
    record: &RunRecord,
    mapping: Option<&dyn FailureMapping>,
    started: Result<Outcome<F>, StartError>,
) -> Ending<F> {
    record.set_executor_running(false);
    match started {
        Ok(outcome) => {
            record.expect_observations_end();
            ending_of(record, mapping, outcome)
        }
        Err(error) => Ending::Failed(failed_of(
            Found::permanent("start_refused", error.to_string()),
            mapping,
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
    let started = start(
        Arc::clone(&ctx.plan.graph),
        ready.state,
        ready.nodes,
        config,
    )
    .await;
    after_start(&ctx.record, ctx.plan.mapping.as_deref(), started)
}

pub(super) async fn body<F: Fields>(ctx: &Ctx<F>, token: CancelToken) -> Ending<F> {
    match AssertUnwindSafe(execute_graph(ctx, token))
        .catch_unwind()
        .await
    {
        Ok(ending) => ending,
        Err(payload) => {
            ctx.record.set_executor_running(false);
            ctx.record.cancel.trigger();
            Ending::Failed(failed_of(
                Found::permanent("panic", crate::panic::text_of(payload)),
                ctx.plan.mapping.as_deref(),
            ))
        }
    }
}

#[cfg(test)]
mod tests;
