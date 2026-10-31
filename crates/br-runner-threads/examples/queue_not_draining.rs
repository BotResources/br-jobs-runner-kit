use br_runner_threads::test_double;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_agentic_contract::{PortError, ThreadAccess, ThreadRef};
use br_llm_messages::{AgentId, Author, EntryId, IncomingEntry, InputTurn, RuntimeNotice, TurnId};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{
    Cause, Error, ItemKind, MemoryQueueStore, QueueObserver, QueueReport, ReportKind,
    ThreadAccessAdapter, ThreadAccessSettings, ThreadClient, WriteContext, WriteContextFn,
};
use uuid::Uuid;

#[derive(Default)]
pub struct CollectedReports {
    reports: Mutex<Vec<QueueReport>>,
}

impl CollectedReports {
    pub fn of_kind(&self, kind: ReportKind) -> Vec<QueueReport> {
        let reports = self.reports.lock().expect("the reports lock");
        reports.iter().filter(|r| r.kind == kind).cloned().collect()
    }
}

impl QueueObserver for CollectedReports {
    fn report(&self, report: QueueReport) {
        self.reports.lock().expect("the reports lock").push(report);
    }
}

#[derive(Debug, PartialEq)]
pub struct StuckThread {
    pub thread: ThreadRef,
    pub waiting: usize,
    pub message_id: Uuid,
    pub item_kind: ItemKind,
    pub last_error: Option<Error>,
}

pub fn stuck_threads(
    adapter: &ThreadAccessAdapter,
    waiting_longer_than: Duration,
) -> Vec<StuckThread> {
    let mut stuck = Vec::new();
    for state in adapter.queue_state().threads {
        let Some(oldest) = state.oldest else { continue };
        if oldest.accepted_at.elapsed() >= waiting_longer_than {
            stuck.push(StuckThread {
                thread: state.thread,
                waiting: state.waiting,
                message_id: oldest.message_id,
                item_kind: oldest.kind,
                last_error: state.last_failure.map(|failed| failed.error),
            });
        }
    }
    stuck
}

pub async fn adapter_observed_by(
    client: &ThreadClient,
    observer: Arc<CollectedReports>,
) -> Result<ThreadAccessAdapter, Error> {
    let agent = AgentId::from_uuid(Uuid::now_v7()).expect("a UUIDv7 is an agent id");
    let write_context = WriteContextFn::new(move |_thread| async move {
        Ok::<_, PortError>(WriteContext {
            agent,
            author: Author::new("Analyst").expect("an author"),
            job_id: None,
            acting_user_id: None,
            impersonator_id: None,
        })
    });
    let settings = ThreadAccessSettings {
        outcome_wait: Duration::from_secs(10),
        retry_wait: Duration::from_millis(50),
        keepalive_interval: Duration::from_secs(30),
        store: Arc::new(MemoryQueueStore::new()),
        observer,
        write_context,
    };
    ThreadAccessAdapter::new(client, settings).await
}

pub async fn accept_an_input_turn(adapter: &ThreadAccessAdapter, thread: &ThreadRef) -> Uuid {
    let entry = EntryId::from_uuid(Uuid::now_v7()).expect("an entry id");
    let notice = RuntimeNotice::new(entry, "the run starts").expect("a notice");
    let turn_id = TurnId::from_uuid(Uuid::now_v7()).expect("a turn id");
    let turn = InputTurn::new(turn_id, vec![IncomingEntry::from(notice)]).expect("an input turn");
    adapter
        .add_input_turn(thread, turn)
        .await
        .expect("accepted");
    turn_id.as_uuid()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let threads = MemoryThreads::new();
    threads.answer_commands(|_| {
        Err(Error::Unavailable(test_double::failure(
            Cause::Transport,
            "the bus is down",
        )))
    });
    let observer = Arc::new(CollectedReports::default());
    let adapter = adapter_observed_by(&threads.client(), observer.clone()).await?;
    let thread = ThreadRef::from_uuid(Uuid::now_v7())?;
    accept_an_input_turn(&adapter, &thread).await;
    while observer.of_kind(ReportKind::AttemptFailed).is_empty() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for stuck in stuck_threads(&adapter, Duration::ZERO) {
        println!("{stuck:?}");
    }
    drop(adapter);
    tokio::time::sleep(Duration::from_millis(100)).await;
    println!("unsent: {}", observer.of_kind(ReportKind::Unsent).len());
    Ok(())
}
