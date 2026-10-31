use std::sync::{Arc, Mutex};

use br_agentic_contract::{
    AgentLabel, AgentPort, Fields, InboxContext, InstanceId, Reason, ThreadRef,
};
use br_llm_messages::{AgentId, EntryId};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::adapter::{InboxPortAdapter, MarkGate, NodeEntry, book_lookup, mark_gate};
use crate::book::tests::{id, scripted, settings, work};
use crate::book::{AgentBook, RestoredAgent};
use crate::queue::{DeliveryQueue, Gate, QueueObserver, QueueReport, QueueSettings};
use crate::test_double::ScriptedJobs;

pub(in crate::adapter) const NODE: &str = "node";

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(in crate::adapter) struct Snap;

impl Fields for Snap {
    type Update = ();

    fn empty_update() -> Self::Update {}

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, _update: Self::Update) -> Result<(), Reason> {
        Ok(())
    }
}

pub(in crate::adapter) fn instance() -> InstanceId {
    InstanceId::new(id(900)).expect("an instance id")
}

pub(in crate::adapter) fn label() -> AgentLabel {
    AgentLabel::new("writer").expect("a label")
}

pub(in crate::adapter) fn thread() -> ThreadRef {
    ThreadRef::from_uuid(id(901)).expect("a thread")
}

#[derive(Default)]
pub(in crate::adapter) struct Reports(Mutex<Vec<QueueReport>>);

impl Reports {
    pub(in crate::adapter) fn all(&self) -> Vec<QueueReport> {
        self.0.lock().expect("lock").clone()
    }
}

impl QueueObserver for Reports {
    fn report(&self, report: &QueueReport) {
        self.0.lock().expect("lock").push(report.clone());
    }
}

pub(in crate::adapter) struct ManualGate {
    end: watch::Sender<Option<Gate>>,
    asked: Arc<Mutex<Vec<(ThreadRef, EntryId)>>>,
}

impl ManualGate {
    pub(in crate::adapter) fn new() -> Self {
        Self {
            end: watch::channel(None).0,
            asked: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(in crate::adapter) fn gate(&self) -> MarkGate {
        let end = self.end.clone();
        let asked = Arc::clone(&self.asked);
        mark_gate(move |thread, entry| {
            asked.lock().expect("lock").push((thread, entry));
            let mut receiver = end.subscribe();
            async move {
                loop {
                    let ended = *receiver.borrow_and_update();
                    if let Some(gate) = ended {
                        return gate;
                    }
                    if receiver.changed().await.is_err() {
                        std::future::pending::<()>().await;
                    }
                }
            }
        })
    }

    pub(in crate::adapter) fn end(&self, gate: Gate) {
        self.end.send_replace(Some(gate));
    }

    pub(in crate::adapter) fn asked(&self) -> Vec<(ThreadRef, EntryId)> {
        self.asked.lock().expect("lock").clone()
    }
}

pub(in crate::adapter) struct World {
    pub jobs: Arc<ScriptedJobs>,
    pub queue: DeliveryQueue,
    pub book: Arc<AgentBook>,
    pub entry: NodeEntry,
    pub reports: Arc<Reports>,
}

pub(in crate::adapter) fn restored_writer() -> RestoredAgent {
    RestoredAgent {
        key: crate::book::AgentKey::new(NODE, "writer"),
        agent_id: id(70),
        has_inbox: true,
    }
}

impl World {
    pub(in crate::adapter) async fn new(gate: MarkGate) -> Self {
        Self::build(scripted(), Vec::new(), gate).await
    }

    pub(in crate::adapter) async fn build(
        jobs: Arc<ScriptedJobs>,
        restored: Vec<RestoredAgent>,
        gate: MarkGate,
    ) -> Self {
        let reports = Arc::new(Reports::default());
        let work = work(&jobs);
        let queue_settings = QueueSettings {
            observer: Arc::clone(&reports) as Arc<dyn QueueObserver>,
            ..settings()
        };
        let (queue, _started) = work.delivery_queue(queue_settings).await.expect("a queue");
        let book = Arc::new(work.agent_book(&queue, restored, false).expect("a book"));
        let lookup = {
            let book = Arc::clone(&book);
            book_lookup(move |_| {
                let book = Arc::clone(&book);
                async move { Ok(book) }
            })
        };
        let entry = NodeEntry::with_inbox("node", lookup, label(), gate);
        Self {
            jobs,
            queue,
            book,
            entry,
            reports,
        }
    }

    pub(in crate::adapter) fn inbox(&self) -> InboxPortAdapter {
        self.entry.inbox_port().expect("a node with an inbox")
    }

    pub(in crate::adapter) async fn agent(&self) -> AgentId {
        self.entry
            .agent_port()
            .agent(&Snap, instance(), &label())
            .await
            .expect("an agent")
    }

    pub(in crate::adapter) async fn registered_agent(&self) -> AgentId {
        let agent = self.agent().await;
        super::drained(&self.queue).await;
        agent
    }

    pub(in crate::adapter) fn context(&self, agent: AgentId) -> InboxContext {
        InboxContext {
            instance: instance(),
            thread: thread(),
            agent,
        }
    }
}
