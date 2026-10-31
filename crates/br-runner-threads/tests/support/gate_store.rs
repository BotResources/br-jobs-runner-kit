use std::sync::Arc;
use std::sync::Mutex;

use br_agentic_contract::{PortFuture, ThreadRef};
use br_runner_threads::{Error, MemoryQueueStore, QueueStore, StoredItem};
use tokio::sync::{Notify, Semaphore};
use uuid::Uuid;

pub struct GateStore {
    inner: MemoryQueueStore,
    free: Mutex<usize>,
    entered: Notify,
    gate: Semaphore,
}

impl GateStore {
    pub fn after_free_appends(free: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: MemoryQueueStore::new(),
            free: Mutex::new(free),
            entered: Notify::new(),
            gate: Semaphore::new(0),
        })
    }

    pub async fn a_gated_append_started(&self) {
        self.entered.notified().await;
    }

    pub fn release_appends(&self) {
        self.gate.add_permits(1024);
    }

    pub async fn kept(&self) -> Vec<StoredItem> {
        self.inner.list().await.expect("the memory store lists")
    }
}

impl QueueStore for GateStore {
    fn append<'a>(&'a self, item: StoredItem) -> PortFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            let free = {
                let mut left = self.free.lock().expect("lock");
                let free = *left > 0;
                *left = left.saturating_sub(1);
                free
            };
            if !free {
                self.entered.notify_one();
                self.gate
                    .acquire()
                    .await
                    .expect("the gate stays open")
                    .forget();
            }
            self.inner.append(item).await
        })
    }

    fn list<'a>(&'a self) -> PortFuture<'a, Result<Vec<StoredItem>, Error>> {
        self.inner.list()
    }

    fn remove<'a>(&'a self, thread: &'a ThreadRef, id: Uuid) -> PortFuture<'a, Result<(), Error>> {
        self.inner.remove(thread, id)
    }
}
