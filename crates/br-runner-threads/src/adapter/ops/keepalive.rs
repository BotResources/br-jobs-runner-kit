use contract_threads::Chunk;
use std::sync::Arc;
use tokio::time::Instant;
use uuid::Uuid;

use crate::adapter::inner::{Inner, KeepaliveTask, Slot, locked};
use crate::queue::item::ItemKind;
use crate::queue::{QueueReport, ReportKind};

async fn queue_emptied(slot: &Slot) {
    loop {
        let mut changes = slot.changed.subscribe();
        if locked(&slot.queue).items.is_empty() || changes.changed().await.is_err() {
            return;
        }
    }
}

impl Inner {
    pub(crate) fn start_keepalives(
        self: &Arc<Self>,
        slot: &Arc<Slot>,
        message: Uuid,
    ) -> Option<KeepaliveTask> {
        if self.stopped.load(std::sync::atomic::Ordering::SeqCst) {
            return None;
        }
        let task = tokio::spawn(Self::keepalives(
            Arc::clone(self),
            Arc::clone(slot),
            message,
        ));
        let handle = task.abort_handle();
        let mut aborts = locked(&self.aborts);
        aborts.retain(|held| !held.is_finished());
        aborts.push(handle.clone());
        drop(aborts);
        if self.stopped.load(std::sync::atomic::Ordering::SeqCst) {
            task.abort();
            return None;
        }
        Some(KeepaliveTask(handle))
    }

    async fn due_at(&self, slot: &Slot, message: Uuid) -> Option<Instant> {
        let mem = slot.mem.lock().await;
        let state = mem.turns.get(&message).filter(|state| !state.closed)?;
        Some(state.last_item_at + self.keepalive_interval)
    }

    async fn keepalives(self: Arc<Self>, slot: Arc<Slot>, message: Uuid) {
        loop {
            if self.stopped.load(std::sync::atomic::Ordering::SeqCst)
                || self.message_refused(message)
            {
                return;
            }
            let Some(due) = self.due_at(&slot, message).await else {
                return;
            };
            let remaining = due.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                tokio::time::sleep(remaining).await;
                continue;
            }
            queue_emptied(&slot).await;
            self.queue_keepalive(&slot, message).await;
        }
    }

    async fn queue_keepalive(self: &Arc<Self>, slot: &Arc<Slot>, message: Uuid) {
        let _ = self
            .ordered(slot, move |inner, slot| async move {
                let mut mem = slot.mem.lock().await;
                let due = mem.turns.get(&message).is_some_and(|state| {
                    !state.closed && state.last_item_at + inner.keepalive_interval <= Instant::now()
                });
                let idle = locked(&slot.queue).items.is_empty();
                if !due || !idle || inner.message_refused(message) {
                    return Ok(());
                }
                let id = Uuid::now_v7();
                let kept = inner
                    .push_chunk(&slot, &mut mem, message, Vec::new(), Chunk::Keepalive, id)
                    .await;
                if let Err(error) = kept {
                    if let Some(state) = mem.turns.get_mut(&message) {
                        state.last_item_at = Instant::now();
                    }
                    inner.report(QueueReport {
                        kind: ReportKind::ItemRefused,
                        thread: slot.thread,
                        message_id: message,
                        item_kind: ItemKind::Chunk,
                        item_id: id,
                        error: Some(error),
                    });
                }
                Ok(())
            })
            .await;
    }
}
