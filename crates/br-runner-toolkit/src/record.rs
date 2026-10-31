use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use br_graph_contract::{CancelHandle, InstanceId};
use br_runner_jobs::{AgentBook, DeliveryQueue};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::context::RunContext;
use crate::hook::InstanceEnded;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Live,
    ExecutorReturned,
    Aborted,
}

struct InstanceInfo {
    node: String,
    hooked: bool,
}

struct Inner {
    phase: Phase,
    book: Option<Arc<AgentBook>>,
    queue: Option<Arc<DeliveryQueue>>,
    context: Option<RunContext>,
    instances: HashMap<InstanceId, InstanceInfo>,
    arrival: Option<tokio::task::JoinHandle<()>>,
}

pub(crate) struct RunRecord {
    pub(crate) run_id: Uuid,
    pub(crate) job_id: Uuid,
    pub(crate) cancel: CancelHandle,
    started_pending: AtomicBool,
    executor_running: AtomicBool,
    inner: Mutex<Inner>,
    outstanding_hooks: AtomicUsize,
    hooks_changed: Notify,
}

impl RunRecord {
    pub(crate) fn new(run_id: Uuid, job_id: Uuid, cancel: CancelHandle) -> Arc<Self> {
        Arc::new(Self {
            run_id,
            job_id,
            cancel,
            started_pending: AtomicBool::new(true),
            executor_running: AtomicBool::new(false),
            inner: Mutex::new(Inner {
                phase: Phase::Live,
                book: None,
                queue: None,
                context: None,
                instances: HashMap::new(),
                arrival: None,
            }),
            outstanding_hooks: AtomicUsize::new(0),
            hooks_changed: Notify::new(),
        })
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn started_pending(&self) -> bool {
        self.started_pending.load(Ordering::SeqCst)
    }

    pub(crate) fn started_ended(&self) {
        self.started_pending.store(false, Ordering::SeqCst);
    }

    pub(crate) fn executor_running(&self) -> bool {
        self.executor_running.load(Ordering::SeqCst)
    }

    pub(crate) fn set_executor_running(&self, running: bool) {
        self.executor_running.store(running, Ordering::SeqCst);
    }

    pub(crate) fn phase(&self) -> Phase {
        self.inner().phase
    }

    pub(crate) fn set_phase(&self, phase: Phase) {
        self.inner().phase = phase;
    }

    pub(crate) fn set_queue(&self, queue: Arc<DeliveryQueue>) {
        self.inner().queue = Some(queue);
    }

    pub(crate) fn queue(&self) -> Option<Arc<DeliveryQueue>> {
        self.inner().queue.clone()
    }

    pub(crate) fn take_queue(&self) -> Option<Arc<DeliveryQueue>> {
        self.inner().queue.take()
    }

    pub(crate) fn set_book(&self, book: Arc<AgentBook>) {
        self.inner().book = Some(book);
    }

    pub(crate) fn book(&self) -> Option<Arc<AgentBook>> {
        self.inner().book.clone()
    }

    pub(crate) fn take_book(&self) -> Option<Arc<AgentBook>> {
        self.inner().book.take()
    }

    pub(crate) fn set_arrival(&self, handle: tokio::task::JoinHandle<()>) {
        self.inner().arrival = Some(handle);
    }

    pub(crate) fn stop_arrival(&self) {
        if let Some(handle) = self.inner().arrival.take() {
            handle.abort();
        }
    }

    pub(crate) fn set_context(&self, context: RunContext) {
        self.inner().context = Some(context);
    }

    pub(crate) fn context(&self) -> Option<RunContext> {
        self.inner().context.clone()
    }

    pub(crate) fn instance_created(&self, id: InstanceId, node: String) {
        self.inner().instances.insert(
            id,
            InstanceInfo {
                node,
                hooked: false,
            },
        );
    }

    pub(crate) fn unhooked_instances(&self) -> Vec<(InstanceId, String)> {
        self.inner()
            .instances
            .iter()
            .filter(|(_, info)| !info.hooked)
            .map(|(id, info)| (*id, info.node.clone()))
            .collect()
    }

    pub(crate) fn claim_hook(&self, id: InstanceId) -> Option<InstanceEnded> {
        let mut inner = self.inner();
        let book = inner.book.clone();
        let info = inner.instances.get_mut(&id)?;
        if info.hooked {
            return None;
        }
        info.hooked = true;
        let node = info.node.clone();
        let agents = book
            .map(|book| {
                book.agents()
                    .into_iter()
                    .filter(|agent| agent.key.node == node)
                    .collect()
            })
            .unwrap_or_default();
        Some(InstanceEnded {
            instance_id: id,
            node,
            agents,
        })
    }

    pub(crate) fn hook_started(&self) {
        self.outstanding_hooks.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn hook_returned(&self) {
        self.outstanding_hooks.fetch_sub(1, Ordering::SeqCst);
        self.hooks_changed.notify_waiters();
    }

    pub(crate) fn outstanding_hooks(&self) -> usize {
        self.outstanding_hooks.load(Ordering::SeqCst)
    }

    pub(crate) async fn hooks_returned(&self) {
        loop {
            let changed = self.hooks_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.outstanding_hooks() == 0 {
                return;
            }
            changed.await;
        }
    }
}
