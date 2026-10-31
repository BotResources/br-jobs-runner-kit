use std::panic::{AssertUnwindSafe, catch_unwind};

use br_graph_contract::{Fields, State};
use br_runner_jobs::{BookedAgent, CallMode, Content, DeliveryQueue, Failed, WriteEnd};

use super::Ctx;
use crate::failure::{Found, failed_of};
use crate::saved_state;

pub(super) enum Ending<F: Fields> {
    Ended(State<F>),
    Failed(Failed),
    Cancelled,
}

pub(crate) enum Terminal {
    Completed {
        answer: Option<WriteEnd>,
        saved: Option<WriteEnd>,
        completed: WriteEnd,
    },
    Failed(WriteEnd),
    None,
}

fn answer_of<F: Fields>(ctx: &Ctx<F>, state: &State<F>) -> Result<Content, Found> {
    let made = match &ctx.plan.answer {
        Some(answer) => catch_unwind(AssertUnwindSafe(|| answer(state)))
            .map_err(|payload| Found::permanent("panic", crate::panic::text_of(payload)))?,
        None => state
            .to_json()
            .map_err(|error| error.to_string())
            .and_then(|json| String::from_utf8(json).map_err(|error| error.to_string()))
            .map(|message| Content {
                message,
                attachments: Vec::new(),
            }),
    };
    made.map_err(|text| Found::permanent("answer_not_made", text))
}

fn made_writes<F: Fields>(
    ctx: &Ctx<F>,
    state: &State<F>,
    agents: &[BookedAgent],
) -> Result<(Option<Content>, Option<serde_json::Value>), Found> {
    let answer = if ctx.work.call() == Some(CallMode::Sync) {
        Some(answer_of(ctx, state)?)
    } else {
        None
    };
    let saved = if ctx.plan.can_restart && agents.iter().any(|agent| agent.has_inbox) {
        Some(
            saved_state::encode(state, agents)
                .map_err(|text| Found::permanent("saved_state_not_made", text))?,
        )
    } else {
        None
    };
    Ok((answer, saved))
}

pub(super) fn hand_over<F: Fields>(
    ctx: &Ctx<F>,
    queue: &DeliveryQueue,
    agents: &[BookedAgent],
    ending: Ending<F>,
) -> Terminal {
    let fail = |found: Found| {
        Terminal::Failed(queue.failed(failed_of(found, ctx.plan.mapping.as_deref())))
    };
    match ending {
        Ending::Cancelled => Terminal::None,
        Ending::Failed(failed) => Terminal::Failed(queue.failed(failed)),
        Ending::Ended(state) => match made_writes(ctx, &state, agents) {
            Err(found) => fail(found),
            Ok((answer, saved)) => {
                let answer = answer.map(|content| queue.answer(content));
                let saved = saved.map(|value| queue.saved_state(value));
                Terminal::Completed {
                    answer,
                    saved,
                    completed: queue.completed(),
                }
            }
        },
    }
}
