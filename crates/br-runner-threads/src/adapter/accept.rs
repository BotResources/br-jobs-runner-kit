use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use super::deliver;
use super::inner::{Inner, Queued, Slot, locked};
use crate::error::Error;
use crate::queue::item::Item;
use crate::queue::{QueueReport, ReportKind};

impl Inner {
    pub(crate) async fn accept(
        self: &Arc<Self>,
        slot: &Arc<Slot>,
        item: Item,
    ) -> Result<(), Error> {
        if self.stopped.load(Ordering::SeqCst) {
            return Err(Error::Store("the adapter is stopped".to_owned()));
        }
        let stored = item.stored(slot.thread)?;
        self.store.append(stored).await.map_err(Error::from_store)?;
        self.enqueue(slot, item);
        Ok(())
    }

    pub(crate) fn enqueue(self: &Arc<Self>, slot: &Arc<Slot>, item: Item) {
        let no = self.numbers.fetch_add(1, Ordering::SeqCst) + 1;
        let spawn = {
            let mut queue = locked(&slot.queue);
            if self.stopped.load(Ordering::SeqCst) {
                drop(queue);
                self.report(QueueReport {
                    kind: ReportKind::Unsent,
                    thread: slot.thread,
                    message_id: item.message,
                    item_kind: item.kind,
                    item_id: item.id,
                    error: None,
                });
                return;
            }
            for entry in &item.entries {
                queue.dropped.remove(entry);
            }
            queue.items.push_back(Queued {
                item,
                no,
                accepted_at: Instant::now(),
            });
            queue.last_no = no;
            !std::mem::replace(&mut queue.worker_running, true)
        };
        slot.bump();
        if spawn {
            let task = tokio::spawn(deliver::run(Arc::clone(self), Arc::clone(slot)));
            let mut aborts = locked(&self.aborts);
            aborts.retain(|handle| !handle.is_finished());
            aborts.push(task.abort_handle());
            if self.stopped.load(Ordering::SeqCst) {
                task.abort();
            }
        }
    }
}
