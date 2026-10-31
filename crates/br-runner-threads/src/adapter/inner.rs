use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use br_agentic_contract::ThreadRef;
use br_llm_messages::{AgentId, EntryId};
use contract_threads::Chunk;
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use super::settings::WriteContextFn;
use crate::client::ThreadClient;
use crate::queue::item::Item;
use crate::queue::{FailedAttempt, QueueReport, QueueStore};
use crate::response::ResponseControl;

pub(crate) fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone)]
pub(crate) struct Queued {
    pub(crate) item: Item,
    pub(crate) no: u64,
    pub(crate) accepted_at: Instant,
}

#[derive(Default)]
pub(crate) struct QueueInner {
    pub(crate) items: VecDeque<Queued>,
    pub(crate) last_failure: Option<FailedAttempt>,
    pub(crate) dropped: HashSet<Uuid>,
    pub(crate) worker_running: bool,
    pub(crate) last_no: u64,
}

pub(crate) struct TurnState {
    pub(crate) agent: AgentId,
    pub(crate) chunks: Vec<Chunk>,
    pub(crate) closed: bool,
}

pub(crate) struct OpenResponse {
    pub(crate) agent: AgentId,
    pub(crate) control: Arc<ResponseControl>,
}

#[derive(Default)]
pub(crate) struct Mem {
    pub(crate) turns: HashMap<Uuid, TurnState>,
    pub(crate) responses: HashMap<EntryId, OpenResponse>,
}

pub(crate) struct Slot {
    pub(crate) thread: ThreadRef,
    pub(crate) mem: tokio::sync::Mutex<Mem>,
    pub(crate) queue: Mutex<QueueInner>,
    pub(crate) changed: watch::Sender<u64>,
}

impl Slot {
    pub(crate) fn new(thread: ThreadRef) -> Arc<Self> {
        Arc::new(Self {
            thread,
            mem: tokio::sync::Mutex::new(Mem::default()),
            queue: Mutex::new(QueueInner::default()),
            changed: watch::channel(0).0,
        })
    }

    pub(crate) fn bump(&self) {
        self.changed.send_modify(|version| *version += 1);
    }
}

pub(crate) struct Inner {
    pub(crate) client: ThreadClient,
    pub(crate) outcome_wait: Duration,
    pub(crate) retry_wait: Duration,
    pub(crate) store: Arc<dyn QueueStore>,
    pub(crate) write_context: WriteContextFn,
    pub(crate) slots: Mutex<HashMap<ThreadRef, Arc<Slot>>>,
    pub(crate) numbers: AtomicU64,
    pub(crate) refused: Mutex<HashSet<Uuid>>,
    pub(crate) stopped: AtomicBool,
    pub(crate) reports: mpsc::UnboundedSender<QueueReport>,
    pub(crate) aborts: Mutex<Vec<tokio::task::AbortHandle>>,
}

impl Inner {
    pub(crate) fn slot(&self, thread: ThreadRef) -> Arc<Slot> {
        Arc::clone(
            locked(&self.slots)
                .entry(thread)
                .or_insert_with(|| Slot::new(thread)),
        )
    }

    pub(crate) fn slots(&self) -> Vec<Arc<Slot>> {
        locked(&self.slots).values().cloned().collect()
    }

    pub(crate) fn report(&self, report: QueueReport) {
        let _ = self.reports.send(report);
    }
}
