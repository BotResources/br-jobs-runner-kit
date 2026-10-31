use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use br_graph_contract::CancelToken;
use br_runner_jobs::{PresenceStatus, QueueObserver, TriggerFeed};
use tokio::sync::{Notify, watch};
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::calls::Calls;
use crate::hook::LifecycleHook;
use crate::instances::Instances;
use crate::record::RunRecord;
use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    NoEntry,
    Push,
    Claim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoopState {
    pub mode: Mode,
    pub replay_complete: bool,
}

impl LoopState {
    pub(crate) fn can_ask(&self) -> bool {
        self.mode != Mode::NoEntry && self.replay_complete
    }
}

#[derive(Default)]
pub(crate) struct Registry {
    runs: HashMap<Uuid, Arc<RunRecord>>,
    known_cancels: HashSet<Uuid>,
}

#[derive(Default)]
pub(crate) struct Closings {
    entries: Mutex<HashMap<Uuid, (AbortHandle, Arc<RunRecord>)>>,
    changed: Notify,
}

impl Closings {
    fn entries(&self) -> MutexGuard<'_, HashMap<Uuid, (AbortHandle, Arc<RunRecord>)>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn add(&self, run_id: Uuid, handle: AbortHandle, record: Arc<RunRecord>) {
        self.entries().insert(run_id, (handle, record));
    }

    pub(crate) fn remove(&self, run_id: Uuid) {
        self.entries().remove(&run_id);
        self.changed.notify_waiters();
    }

    pub(crate) fn drain_all(&self) -> Vec<(AbortHandle, Arc<RunRecord>)> {
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

pub(crate) struct Shared {
    pub calls: Calls,
    pub settings: Settings,
    pub hook: Option<LifecycleHook>,
    pub author_observer: Option<Arc<dyn QueueObserver>>,
    pub drain: CancellationToken,
    pub status: watch::Sender<PresenceStatus>,
    pub beat: watch::Sender<u64>,
    pub loop_state: watch::Sender<LoopState>,
    pub instances: Arc<Instances>,
    pub closings: Closings,
    pub triggers: tokio::sync::Mutex<Option<TriggerFeed>>,
    pub finished: watch::Sender<usize>,
    pub host_cancel: watch::Sender<bool>,
    claims_stopped: AtomicBool,
    single_run: AtomicBool,
    taken: AtomicUsize,
    registry: Mutex<Registry>,
}

impl Shared {
    pub(crate) fn new(
        calls: Calls,
        settings: Settings,
        hook: Option<LifecycleHook>,
        author_observer: Option<Arc<dyn QueueObserver>>,
        instances: Arc<Instances>,
    ) -> Arc<Self> {
        Arc::new(Self {
            calls,
            settings,
            hook,
            author_observer,
            drain: CancellationToken::new(),
            status: watch::channel(PresenceStatus::Ready).0,
            beat: watch::channel(0).0,
            loop_state: watch::channel(LoopState {
                mode: Mode::NoEntry,
                replay_complete: false,
            })
            .0,
            instances,
            closings: Closings::default(),
            triggers: tokio::sync::Mutex::new(None),
            finished: watch::channel(0).0,
            host_cancel: watch::channel(false).0,
            claims_stopped: AtomicBool::new(false),
            single_run: AtomicBool::new(false),
            taken: AtomicUsize::new(0),
            registry: Mutex::new(Registry::default()),
        })
    }

    fn registry(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn draining(&self) -> bool {
        self.drain.is_cancelled()
    }

    pub(crate) fn register_run(&self, run_id: Uuid, job_id: Uuid) -> (Arc<RunRecord>, CancelToken) {
        let token = CancelToken::new();
        self.taken.fetch_add(1, Ordering::SeqCst);
        let record = RunRecord::new(run_id, job_id, token.handle().clone());
        let mut registry = self.registry();
        if registry.known_cancels.contains(&run_id) || *self.host_cancel.borrow() {
            token.handle().trigger();
        }
        registry.runs.insert(run_id, Arc::clone(&record));
        (record, token)
    }

    pub(crate) fn run(&self, run_id: Uuid) -> Option<Arc<RunRecord>> {
        self.registry().runs.get(&run_id).cloned()
    }

    pub(crate) fn runs(&self) -> Vec<Arc<RunRecord>> {
        self.registry().runs.values().cloned().collect()
    }

    pub(crate) fn forget_run(&self, run_id: Uuid) {
        self.registry().runs.remove(&run_id);
        self.finished.send_modify(|count| *count += 1);
    }

    pub(crate) fn order_made(&self, run_id: Uuid) {
        let mut registry = self.registry();
        registry.known_cancels.insert(run_id);
        if let Some(record) = registry.runs.get(&run_id) {
            record.cancel.trigger();
        }
    }

    pub(crate) fn order_deleted(&self, run_id: Uuid) {
        self.registry().known_cancels.remove(&run_id);
    }

    pub(crate) fn replace_orders(&self, orders: HashSet<Uuid>) {
        let mut registry = self.registry();
        for run_id in &orders {
            if let Some(record) = registry.runs.get(run_id) {
                record.cancel.trigger();
            }
        }
        registry.known_cancels = orders;
    }

    #[cfg(feature = "test-host")]
    pub(crate) fn cancel_all_for_host(&self) {
        self.host_cancel.send_replace(true);
        for record in self.runs() {
            record.cancel.trigger();
        }
    }

    #[cfg(feature = "test-host")]
    pub(crate) fn take_one_run_only(&self) {
        self.single_run.store(true, Ordering::SeqCst);
    }

    pub(crate) fn runs_taken(&self) -> usize {
        self.taken.load(Ordering::SeqCst)
    }

    pub(crate) fn single_run(&self) -> bool {
        self.single_run.load(Ordering::SeqCst)
    }

    pub(crate) fn claims_stopped(&self) -> bool {
        self.claims_stopped.load(Ordering::SeqCst)
    }

    pub(crate) fn set_claims_stopped(&self, stopped: bool) {
        self.claims_stopped.store(stopped, Ordering::SeqCst);
    }

    pub(crate) fn beat_now(&self) {
        self.beat.send_modify(|beat| *beat += 1);
    }

    pub(crate) async fn next_beat(&self) {
        let mut beats = self.beat.subscribe();
        let _ = beats.changed().await;
    }
}
