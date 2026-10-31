use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_agentic_contract::{PortError, ThreadAccess};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{
    MemoryQueueStore, QueueObserver, QueueReport, ThreadAccessAdapter, ThreadAccessSettings,
    ThreadClient, WriteContext, WriteContextFn,
};

use super::ids::{agent_id, author};

#[derive(Default)]
pub struct Recorder {
    reports: Mutex<Vec<QueueReport>>,
}

impl Recorder {
    pub fn reports(&self) -> Vec<QueueReport> {
        self.reports.lock().expect("lock").clone()
    }
}

impl QueueObserver for Recorder {
    fn report(&self, report: QueueReport) {
        self.reports.lock().expect("lock").push(report);
    }
}

pub fn write_context(agent: br_llm_messages::AgentId) -> WriteContextFn {
    WriteContextFn::new(move |_thread| async move {
        Ok::<_, PortError>(WriteContext {
            agent,
            author: author("Analyst"),
            job_id: None,
            acting_user_id: None,
            impersonator_id: None,
        })
    })
}

pub struct World {
    pub threads: MemoryThreads,
    pub client: ThreadClient,
    pub store: Arc<MemoryQueueStore>,
    pub recorder: Arc<Recorder>,
    pub adapter: Arc<ThreadAccessAdapter>,
    pub agent: br_llm_messages::AgentId,
}

pub fn settings(
    store: Arc<MemoryQueueStore>,
    recorder: Arc<Recorder>,
    agent: br_llm_messages::AgentId,
) -> ThreadAccessSettings {
    ThreadAccessSettings {
        outcome_wait: Duration::from_secs(5),
        retry_wait: Duration::from_millis(20),
        keepalive_interval: Duration::from_secs(3600),
        store,
        observer: recorder,
        write_context: write_context(agent),
    }
}

pub async fn world() -> World {
    let threads = MemoryThreads::new();
    let client = threads.client();
    let store = Arc::new(MemoryQueueStore::new());
    let recorder = Arc::new(Recorder::default());
    let agent = agent_id();
    let adapter = ThreadAccessAdapter::new(
        &client,
        settings(Arc::clone(&store), Arc::clone(&recorder), agent),
    )
    .await
    .expect("adapter");
    World {
        threads,
        client,
        store,
        recorder,
        adapter: Arc::new(adapter),
        agent,
    }
}

impl World {
    pub fn access(&self) -> &dyn ThreadAccess {
        self.adapter.as_ref()
    }
}

pub async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..400 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never true: {what}");
}

pub async fn settle() {
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
}
