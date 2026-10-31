use super::inner::{Inner, locked};
use crate::queue::{OldestItem, QueueState, ThreadQueueState};

pub(super) fn of(inner: &Inner) -> QueueState {
    let mut threads = Vec::new();
    for slot in inner.slots() {
        let queue = locked(&slot.queue);
        let Some(head) = queue.items.front() else {
            continue;
        };
        threads.push(ThreadQueueState {
            thread: slot.thread,
            waiting: queue.items.len(),
            oldest: Some(OldestItem {
                accepted_at: head.accepted_at,
                message_id: head.item.message,
                kind: head.item.kind,
            }),
            last_failure: queue.last_failure.clone(),
        });
    }
    QueueState { threads }
}
