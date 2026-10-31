use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_agentic_contract::{PortFuture, ThreadRef};
use br_llm_messages::AgentId;
use br_runner_threads::{
    Error, MemoryQueueStore, QueueObserver, QueueReport, QueueStore, StoredItem,
    ThreadAccessAdapter, ThreadAccessSettings, ThreadClient,
};
use uuid::Uuid;

use super::world::write_context;

pub struct FlakyStore {
    inner: MemoryQueueStore,
    appends_left: AtomicUsize,
    list_fails: AtomicBool,
    remove_fails: AtomicBool,
}

impl FlakyStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: MemoryQueueStore::new(),
            appends_left: AtomicUsize::new(usize::MAX),
            list_fails: AtomicBool::new(false),
            remove_fails: AtomicBool::new(false),
        })
    }

    pub fn allow_appends(&self, count: usize) {
        self.appends_left.store(count, Ordering::SeqCst);
    }

    pub fn allow_all_appends(&self) {
        self.appends_left.store(usize::MAX, Ordering::SeqCst);
    }

    pub fn fail_list(&self, fails: bool) {
        self.list_fails.store(fails, Ordering::SeqCst);
    }

    pub fn fail_remove(&self, fails: bool) {
        self.remove_fails.store(fails, Ordering::SeqCst);
    }

    pub async fn kept(&self) -> Vec<StoredItem> {
        self.inner.list().await.expect("the memory store lists")
    }

    pub async fn keep_raw(&self, item: StoredItem) {
        self.inner
            .append(item)
            .await
            .expect("the memory store appends");
    }

    fn take_append(&self) -> bool {
        let left = self.appends_left.load(Ordering::SeqCst);
        if left == 0 {
            return false;
        }
        if left != usize::MAX {
            self.appends_left.store(left - 1, Ordering::SeqCst);
        }
        true
    }
}

impl QueueStore for FlakyStore {
    fn append<'a>(&'a self, item: StoredItem) -> PortFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            if !self.take_append() {
                return Err(Error::Store("the test store refuses to append".to_owned()));
            }
            self.inner.append(item).await
        })
    }

    fn list<'a>(&'a self) -> PortFuture<'a, Result<Vec<StoredItem>, Error>> {
        Box::pin(async move {
            if self.list_fails.load(Ordering::SeqCst) {
                return Err(Error::Store("the test store refuses to list".to_owned()));
            }
            self.inner.list().await
        })
    }

    fn remove<'a>(&'a self, thread: &'a ThreadRef, id: Uuid) -> PortFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            if self.remove_fails.load(Ordering::SeqCst) {
                return Err(Error::Store("the test store refuses to remove".to_owned()));
            }
            self.inner.remove(thread, id).await
        })
    }
}

pub async fn build_adapter(
    client: &ThreadClient,
    store: Arc<dyn QueueStore>,
    observer: Arc<dyn QueueObserver>,
    agent: AgentId,
) -> Result<ThreadAccessAdapter, Error> {
    ThreadAccessAdapter::new(
        client,
        ThreadAccessSettings {
            outcome_wait: Duration::from_secs(5),
            retry_wait: Duration::from_millis(20),
            keepalive_interval: Duration::from_secs(3600),
            store,
            observer,
            write_context: write_context(agent),
        },
    )
    .await
}

pub struct GatedObserver {
    open: Mutex<bool>,
    gate: std::sync::Condvar,
    seen: Mutex<Vec<QueueReport>>,
}

impl GatedObserver {
    pub fn closed() -> Arc<Self> {
        Arc::new(Self {
            open: Mutex::new(false),
            gate: std::sync::Condvar::new(),
            seen: Mutex::new(Vec::new()),
        })
    }

    pub fn open(&self) {
        *self.open.lock().expect("lock") = true;
        self.gate.notify_all();
    }

    pub fn seen(&self) -> Vec<QueueReport> {
        self.seen.lock().expect("lock").clone()
    }
}

impl QueueObserver for GatedObserver {
    fn report(&self, report: QueueReport) {
        let mut open = self.open.lock().expect("lock");
        while !*open {
            open = self.gate.wait(open).expect("wait");
        }
        drop(open);
        self.seen.lock().expect("lock").push(report);
    }
}
