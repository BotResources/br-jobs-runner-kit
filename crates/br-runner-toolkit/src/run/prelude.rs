use std::sync::Arc;

use br_graph_contract::{CancelView, Fields, State};
use br_runner_jobs::{Error, Failed};
use futures::FutureExt;
use std::panic::AssertUnwindSafe;

use super::Ctx;
use super::arrival;
use crate::context::RunContext;
use crate::failure::{Found, failed_of};
use crate::run_start::RunStartInput;
use crate::saved_state::{self, Restored};

pub(super) enum Early {
    Cancelled,
    Failed(Failed),
}

pub(super) struct Ready<F: Fields> {
    pub state: State<F>,
    pub nodes: Vec<String>,
}

fn fail<F: Fields>(ctx: &Ctx<F>, reason_code: &str, text: impl Into<String>) -> Early {
    Early::Failed(failed_of(
        Found::permanent(reason_code, text),
        ctx.plan.mapping.as_deref(),
    ))
}

async fn read_saved_state<F: Fields>(ctx: &Ctx<F>) -> Result<Restored<F>, Early> {
    let saved = match ctx.work.saved_state().await {
        Ok(saved) => saved,
        Err(error @ (Error::Refused(_) | Error::Unreadable(_) | Error::InvalidInput(_))) => {
            return Err(fail(ctx, "saved_state_invalid", error.to_string()));
        }
        Err(error) => {
            return Err(Early::Failed(failed_of(
                Found::transient("saved_state_unavailable", error.to_string()),
                ctx.plan.mapping.as_deref(),
            )));
        }
    };
    saved_state::decode::<F>(saved.state, ctx.plan.state_limit)
        .map_err(|text| fail(ctx, "saved_state_invalid", text))
}

pub(super) async fn prepare<F: Fields>(
    ctx: &Ctx<F>,
    cancel: &CancelView,
) -> Result<Ready<F>, Early> {
    if cancel.is_triggered() {
        return Err(Early::Cancelled);
    }
    let restart = ctx.work.restart().is_some();
    let (restored_fields, restored_agents) = if restart {
        let restored = read_saved_state(ctx).await?;
        (Some(restored.fields), restored.agents)
    } else {
        (None, Vec::new())
    };
    let book = ctx
        .work
        .agent_book(&ctx.queue, restored_agents, ctx.plan.can_restart)
        .map_err(|error| fail(ctx, "saved_state_invalid", error.to_string()))?;
    let book = Arc::new(book);
    ctx.record.set_book(Arc::clone(&book));
    ctx.record.set_arrival(arrival::spawn(
        Arc::clone(&ctx.shared),
        Arc::clone(&ctx.record),
        Arc::clone(&ctx.work),
        Arc::clone(&book),
    ));
    let context = RunContext::new(
        Arc::clone(&ctx.work),
        &ctx.queue,
        &book,
        ctx.shared.calls.clone(),
    );
    ctx.record.set_context(context.clone());
    if cancel.is_triggered() {
        return Err(Early::Cancelled);
    }
    let started = AssertUnwindSafe(ctx.plan.run_start.start(RunStartInput {
        work: Arc::clone(&ctx.work),
        context,
        cancel: cancel.clone(),
        restored: restored_fields,
    }))
    .catch_unwind()
    .await;
    let started = match started {
        Ok(Ok(started)) => started,
        Ok(Err(refusal)) => return Err(Early::Failed(refusal.into_failed())),
        Err(payload) => return Err(fail(ctx, "panic", crate::panic::text_of(payload))),
    };
    if cancel.is_triggered() {
        return Err(Early::Cancelled);
    }
    let created = match ctx.plan.state_limit {
        Some(limit) => State::create_with_limit(started.fields, limit),
        None => State::create(started.fields),
    };
    let state = created.map_err(|error| fail(ctx, "start_refused", error.to_string()))?;
    let nodes = if restart {
        ctx.plan.graph.entries().to_vec()
    } else {
        started.first_nodes
    };
    Ok(Ready { state, nodes })
}
