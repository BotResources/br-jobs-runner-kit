use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::Notify;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::record::RunRecord;

type Entries = HashMap<Uuid, (JoinHandle<()>, Arc<RunRecord>)>;

#[derive(Default)]
pub(crate) struct Closings {
    entries: Mutex<Entries>,
    changed: Notify,
}

impl Closings {
    fn entries(&self) -> MutexGuard<'_, Entries> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn add(&self, run_id: Uuid, handle: JoinHandle<()>, record: Arc<RunRecord>) {
        self.entries().insert(run_id, (handle, record));
    }

    pub(crate) fn remove(&self, run_id: Uuid) {
        self.entries().remove(&run_id);
        self.changed.notify_waiters();
    }

    pub(crate) fn take_all(&self) -> Vec<(JoinHandle<()>, Arc<RunRecord>)> {
        self.entries().drain().map(|(_, entry)| entry).collect()
    }

    pub(crate) async fn all_ended(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.entries().is_empty() {
                return;
            }
            changed.await;
        }
    }
}
