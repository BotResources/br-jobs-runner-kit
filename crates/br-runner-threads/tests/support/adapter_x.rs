use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::time::Duration;

use br_agentic_contract::{PortError, PortFuture, ThreadAccess, ThreadRef};
use br_llm_messages::AgentId;
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{
    Error, MemoryQueueStore, QueueStore, StoredItem, ThreadAccessAdapter, ThreadAccessSettings,
    WriteContext, WriteContextFn,
};
use uuid::Uuid;

use super::ids::{agent_id, author};
use super::world::{Recorder, eventually};

pub fn fault_of<T>(result: Result<T, PortError>) -> Error {
    match result {
        Err(PortError::Fault(cause)) => cause
            .downcast_ref::<Error>()
            .expect("the cause of a fault is the error of this library")
            .clone(),
        Err(PortError::Refused(reason)) => panic!("a refusal, not a fault: {reason}"),
        Ok(_) => panic!("the operation was accepted"),
    }
}

pub struct FlakyStore {
    inner: MemoryQueueStore,
    appends_left: AtomicI64,
    list_fails: AtomicBool,
}

impl FlakyStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: MemoryQueueStore::new(),
            appends_left: AtomicI64::new(i64::MAX),
            list_fails: AtomicBool::new(false),
        })
    }

    pub fn refuse_appends_from_now(&self) {
        self.appends_left.store(0, Ordering::SeqCst);
    }

    pub fn accept_only(&self, appends: i64) {
        self.appends_left.store(appends, Ordering::SeqCst);
    }

    pub fn accept_appends_again(&self) {
        self.appends_left.store(i64::MAX, Ordering::SeqCst);
    }

    pub fn fail_list(&self) {
        self.list_fails.store(true, Ordering::SeqCst);
    }
}

impl QueueStore for FlakyStore {
    fn append<'a>(&'a self, item: StoredItem) -> PortFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            if self.appends_left.fetch_sub(1, Ordering::SeqCst) <= 0 {
                return Err(Error::Store(
                    "the store refuses to keep the item".to_owned(),
                ));
            }
            self.inner.append(item).await
        })
    }

    fn list<'a>(&'a self) -> PortFuture<'a, Result<Vec<StoredItem>, Error>> {
        Box::pin(async move {
            if self.list_fails.load(Ordering::SeqCst) {
                return Err(Error::Store("the store cannot list".to_owned()));
            }
            self.inner.list().await
        })
    }

    fn remove<'a>(&'a self, thread: &'a ThreadRef, id: Uuid) -> PortFuture<'a, Result<(), Error>> {
        Box::pin(async move { self.inner.remove(thread, id).await })
    }
}

pub fn context_of(agent: AgentId) -> WriteContext {
    WriteContext {
        agent,
        author: author("Analyst"),
        job_id: None,
        acting_user_id: None,
        impersonator_id: None,
    }
}

pub fn fixed_context(context: WriteContext) -> WriteContextFn {
    WriteContextFn::new(move |_thread| {
        let context = context.clone();
        async move { Ok::<_, PortError>(context) }
    })
}

pub fn counting_context(agent: AgentId, calls: Arc<AtomicUsize>) -> WriteContextFn {
    WriteContextFn::new(move |_thread| {
        calls.fetch_add(1, Ordering::SeqCst);
        async move { Ok::<_, PortError>(context_of(agent)) }
    })
}

pub fn failing_context(reason: &'static str) -> WriteContextFn {
    WriteContextFn::new(
        move |_thread| async move { Err::<WriteContext, _>(PortError::refused(reason)) },
    )
}

pub struct Rig {
    pub threads: MemoryThreads,
    pub recorder: Arc<Recorder>,
    pub adapter: ThreadAccessAdapter,
    pub agent: AgentId,
}

impl Rig {
    pub fn access(&self) -> &dyn ThreadAccess {
        &self.adapter
    }

    pub async fn idle(&self) {
        eventually("the thread queue is empty", || {
            self.adapter.queue_state().threads.is_empty()
        })
        .await;
    }
}

pub fn settings_for(
    store: Arc<dyn QueueStore>,
    recorder: Arc<Recorder>,
    write_context: WriteContextFn,
) -> ThreadAccessSettings {
    ThreadAccessSettings {
        outcome_wait: Duration::from_secs(5),
        retry_wait: Duration::from_millis(20),
        store,
        observer: recorder,
        write_context,
    }
}

pub async fn rig_over(
    store: Arc<dyn QueueStore>,
    write_context: WriteContextFn,
    agent: AgentId,
) -> Rig {
    rig_on(MemoryThreads::new(), store, write_context, agent).await
}

pub async fn rig_on(
    threads: MemoryThreads,
    store: Arc<dyn QueueStore>,
    write_context: WriteContextFn,
    agent: AgentId,
) -> Rig {
    let recorder = Arc::new(Recorder::default());
    let adapter = ThreadAccessAdapter::new(
        &threads.client(),
        settings_for(store, Arc::clone(&recorder), write_context),
    )
    .await
    .expect("adapter");
    Rig {
        threads,
        recorder,
        adapter,
        agent,
    }
}

pub async fn rig_with_context(context: WriteContext) -> Rig {
    let agent = context.agent;
    rig_over(
        Arc::new(MemoryQueueStore::new()),
        fixed_context(context),
        agent,
    )
    .await
}

pub async fn rig_with_store(store: Arc<dyn QueueStore>) -> Rig {
    let agent = agent_id();
    rig_over(store, fixed_context(context_of(agent)), agent).await
}
