use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use br_runner_jobs::{DeliveryQueue, Removal, WriteEnd};
use futures::FutureExt;
use tokio::sync::oneshot;

use super::finish::Terminal;
use crate::failure::{FailureMapping, Found, failed_of};
use crate::record::RunRecord;
use crate::shared::Shared;

fn cause_text(removal: &Removal) -> Option<String> {
    match removal {
        Removal::Refused(error) | Removal::Failed(error) => Some(error.to_string()),
        _ => None,
    }
}

async fn body(queue: &DeliveryQueue, terminal: Terminal, mapping: Option<&dyn FailureMapping>) {
    match terminal {
        Terminal::None => {}
        Terminal::Failed(end) => {
            let _ = end.await;
        }
        Terminal::Completed {
            answer,
            saved,
            completed,
        } => {
            let answer = await_end(answer).await;
            let saved = await_end(saved).await;
            let completed = completed.await;
            if matches!(completed, Err(Removal::DependsOnRemoved)) {
                let cause = [("answer_refused", answer), ("saved_state_refused", saved)]
                    .into_iter()
                    .find_map(|(code, end)| {
                        end.and_then(|result| result.err())
                            .and_then(|removal| cause_text(&removal))
                            .map(|text| (code, text))
                    });
                if let Some((code, text)) = cause {
                    let failed = failed_of(Found::permanent(code, text), mapping);
                    let _ = queue.failed(failed).await;
                }
            }
        }
    }
    queue.drained().await;
}

async fn await_end(end: Option<WriteEnd>) -> Option<Result<(), Removal>> {
    match end {
        Some(end) => Some(end.await),
        None => None,
    }
}

pub(super) async fn run(
    shared: Arc<Shared>,
    record: Arc<RunRecord>,
    queue: Arc<DeliveryQueue>,
    terminal: Terminal,
    mapping: Option<Arc<dyn FailureMapping>>,
    registered: oneshot::Receiver<()>,
) {
    let _ = registered.await;
    let ended = AssertUnwindSafe(body(&queue, terminal, mapping.as_deref()))
        .catch_unwind()
        .await;
    if ended.is_err() {
        tracing::error!(run_id = %record.run_id, "the closing of the run panicked");
    }
    drop(queue);
    record.take_queue();
    shared.closings.remove(record.run_id);
    shared.forget_run(record.run_id);
}
