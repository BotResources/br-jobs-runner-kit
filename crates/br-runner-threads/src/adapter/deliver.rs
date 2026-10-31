use std::sync::Arc;

use super::inner::{Inner, Queued, Slot, locked};
use crate::backend::Command;
use crate::error::Error;
use crate::message::MessageStatus;
use crate::queue::item::{ItemKind, Payload};
use crate::queue::{FailedAttempt, QueueReport, ReportKind};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ended {
    Delivered,
    Refused,
    Unsent,
}

fn retryable(kind: ItemKind, error: &Error) -> bool {
    match kind {
        ItemKind::Chunk => matches!(error, Error::StreamBroken),
        _ => matches!(
            error,
            Error::Unavailable(_) | Error::Lost(_) | Error::Unreadable(_)
        ),
    }
}

fn report(kind: ReportKind, slot: &Slot, head: &Queued, error: Option<Error>) -> QueueReport {
    QueueReport {
        kind,
        thread: slot.thread,
        message_id: head.item.message,
        item_kind: head.item.kind,
        item_id: head.item.id,
        error,
    }
}

impl Inner {
    async fn attempt(&self, head: &Queued) -> Result<Option<MessageStatus>, Error> {
        let item = &head.item;
        match &item.payload {
            Payload::Command(body) => {
                let command = Command {
                    id: item.id,
                    body: body.clone(),
                };
                let outcome = if item.kind == ItemKind::Closing {
                    self.client
                        .close_numbered(command, self.outcome_wait)
                        .await?
                } else {
                    self.client
                        .open_numbered(command, self.outcome_wait)
                        .await?
                };
                Ok(outcome.status)
            }
            Payload::Chunk { seq, chunk } => {
                self.client
                    .send_numbered(item.message, *seq, chunk.clone(), self.outcome_wait)
                    .await?;
                Ok(None)
            }
        }
    }

    async fn end_item(&self, slot: &Slot, head: &Queued, how: Ended) {
        {
            let mut queue = locked(&slot.queue);
            queue.items.pop_front();
            match how {
                Ended::Delivered => queue.last_failure = None,
                Ended::Refused | Ended::Unsent => queue.dropped.extend(head.item.entries.iter()),
            }
        }
        slot.bump();
        if let Err(error) = self.store.remove(&slot.thread, head.item.id).await {
            self.report(report(
                ReportKind::RemoveFailed,
                slot,
                head,
                Some(Error::Store(error.to_string())),
            ));
        }
    }

    fn message_refused(&self, message: uuid::Uuid) -> bool {
        locked(&self.refused).contains(&message)
    }
}

pub(crate) async fn run(inner: Arc<Inner>, slot: Arc<Slot>) {
    loop {
        let head = {
            let mut queue = locked(&slot.queue);
            match queue.items.front() {
                Some(head) => head.clone(),
                None => {
                    queue.worker_running = false;
                    return;
                }
            }
        };
        if head.item.kind != ItemKind::Opening && inner.message_refused(head.item.message) {
            inner.report(report(ReportKind::ItemDroppedUnsent, &slot, &head, None));
            inner.end_item(&slot, &head, Ended::Unsent).await;
            continue;
        }
        match inner.attempt(&head).await {
            Ok(status) => {
                if head.item.kind == ItemKind::Closing && status == Some(MessageStatus::Failed) {
                    inner.report(report(ReportKind::ClosingFailed, &slot, &head, None));
                }
                inner.end_item(&slot, &head, Ended::Delivered).await;
            }
            Err(error) if retryable(head.item.kind, &error) => {
                locked(&slot.queue).last_failure = Some(FailedAttempt {
                    item_id: head.item.id,
                    error: error.clone(),
                });
                inner.report(report(ReportKind::AttemptFailed, &slot, &head, Some(error)));
                tokio::time::sleep(inner.retry_wait).await;
            }
            Err(error) => {
                let kind = if head.item.kind == ItemKind::Closing {
                    ReportKind::ClosingFailed
                } else {
                    ReportKind::ItemRefused
                };
                inner.report(report(kind, &slot, &head, Some(error)));
                if head.item.kind == ItemKind::Opening {
                    locked(&inner.refused).insert(head.item.message);
                }
                inner.end_item(&slot, &head, Ended::Refused).await;
            }
        }
    }
}
