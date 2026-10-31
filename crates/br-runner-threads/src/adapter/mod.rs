mod accept;
mod deliver;
mod inner;
mod lane;
mod ops;
mod port;
mod settings;
mod state;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use br_agentic_contract::{
    AiTurnRef, PortFuture, ResponseStream, ResultStream, ResultStreamBox, ThreadAccess, ThreadRef,
};
use br_llm_messages::{
    AgentId, Conversation, EntryId, InputTurn, ModelResponse, RuntimeToolResults, TurnClosure,
    TurnEntry,
};
use tokio::sync::mpsc;

use crate::client::ThreadClient;
use crate::error::Error;
use crate::ids::require_wait;
use crate::queue::item::Item;
use crate::queue::{QueueObserver, QueueReport, QueueState, ReportKind};
use inner::{Inner, locked};

pub use ops::EntryOutcome;
pub use port::ThreadPortAdapter;
pub use settings::{ThreadAccessSettings, WriteContext, WriteContextFn};

pub struct ThreadAccessAdapter {
    inner: Arc<Inner>,
}

struct DroppedResult;

impl ResultStream for DroppedResult {
    fn send<'a>(&'a mut self, _fragment: String) -> PortFuture<'a, ()> {
        Box::pin(async {})
    }
}

fn spawn_reporter(observer: Arc<dyn QueueObserver>) -> mpsc::UnboundedSender<QueueReport> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<QueueReport>();
    tokio::spawn(async move {
        while let Some(report) = receiver.recv().await {
            observer.report(report);
        }
    });
    sender
}

impl ThreadAccessAdapter {
    pub async fn new(client: &ThreadClient, settings: ThreadAccessSettings) -> Result<Self, Error> {
        require_wait(settings.outcome_wait, "the outcome wait")?;
        require_wait(settings.retry_wait, "the wait between attempts")?;
        require_wait(settings.keepalive_interval, "the keepalive interval")?;
        let kept = settings.store.list().await.map_err(Error::from_store)?;
        let leftovers = kept
            .iter()
            .map(Item::read)
            .collect::<Result<Vec<_>, Error>>()?;
        let inner = Arc::new(Inner {
            client: client.clone(),
            outcome_wait: settings.outcome_wait,
            retry_wait: settings.retry_wait,
            keepalive_interval: settings.keepalive_interval,
            store: settings.store,
            write_context: settings.write_context,
            slots: Mutex::new(HashMap::new()),
            numbers: AtomicU64::new(0),
            refused: Mutex::new(HashSet::new()),
            broken: Mutex::new(HashSet::new()),
            stopped: AtomicBool::new(false),
            reports: spawn_reporter(settings.observer),
            aborts: Mutex::new(Vec::new()),
        });
        for (stored, item) in kept.iter().zip(leftovers) {
            let slot = inner.slot(stored.thread);
            inner.enqueue(&slot, item);
        }
        Ok(Self { inner })
    }

    pub fn queue_state(&self) -> QueueState {
        state::of(&self.inner)
    }

    pub async fn entry_stored(&self, thread: &ThreadRef, entry: EntryId) -> EntryOutcome {
        self.inner.entry_stored(thread, entry).await
    }

    pub async fn instance_ended(&self, agent: AgentId) {
        self.inner.instance_ended(agent).await
    }
}

impl Drop for ThreadAccessAdapter {
    fn drop(&mut self) {
        let inner = &self.inner;
        inner.stopped.store(true, Ordering::SeqCst);
        for handle in locked(&inner.aborts).drain(..) {
            handle.abort();
        }
        for slot in inner.slots() {
            let queue = locked(&slot.queue);
            for queued in &queue.items {
                inner.report(QueueReport {
                    kind: ReportKind::Unsent,
                    thread: slot.thread,
                    message_id: queued.item.message,
                    item_kind: queued.item.kind,
                    item_id: queued.item.id,
                    error: None,
                });
            }
        }
    }
}

impl ThreadAccess for ThreadAccessAdapter {
    fn read<'a>(
        &'a self,
        thread: &'a ThreadRef,
    ) -> PortFuture<'a, Result<Conversation, br_agentic_contract::PortError>> {
        Box::pin(async move { self.inner.read(thread).await })
    }

    fn add_input_turn<'a>(
        &'a self,
        thread: &'a ThreadRef,
        turn: InputTurn,
    ) -> PortFuture<'a, Result<(), br_agentic_contract::PortError>> {
        Box::pin(async move { self.inner.add_input_turn(thread, turn).await })
    }

    fn add_entry<'a>(
        &'a self,
        thread: &'a ThreadRef,
        turn: AiTurnRef,
        entry: TurnEntry,
    ) -> PortFuture<'a, Result<(), br_agentic_contract::PortError>> {
        Box::pin(async move { self.inner.add_entry(thread, turn, entry).await })
    }

    fn close_turn<'a>(
        &'a self,
        thread: &'a ThreadRef,
        turn: AiTurnRef,
        closure: TurnClosure,
    ) -> PortFuture<'a, Result<(), br_agentic_contract::PortError>> {
        Box::pin(async move { self.inner.close_turn(thread, turn, closure).await })
    }

    fn open_response<'a>(
        &'a self,
        thread: &'a ThreadRef,
        turn: AiTurnRef,
        entry: EntryId,
    ) -> PortFuture<'a, Result<ResponseStream, br_agentic_contract::PortError>> {
        Box::pin(async move { self.inner.open_response(thread, turn, entry).await })
    }

    fn complete_response<'a>(
        &'a self,
        thread: &'a ThreadRef,
        turn: AiTurnRef,
        response: ModelResponse,
    ) -> PortFuture<'a, Result<(), br_agentic_contract::PortError>> {
        Box::pin(async move { self.inner.complete_response(thread, turn, response).await })
    }

    fn discard_response<'a>(
        &'a self,
        thread: &'a ThreadRef,
        entry: EntryId,
    ) -> PortFuture<'a, Result<(), br_agentic_contract::PortError>> {
        Box::pin(async move { self.inner.discard_response(thread, entry).await })
    }

    fn open_result<'a>(
        &'a self,
        thread: &'a ThreadRef,
        turn: AiTurnRef,
        _entry: EntryId,
    ) -> PortFuture<'a, Result<ResultStreamBox, br_agentic_contract::PortError>> {
        Box::pin(async move {
            self.inner.open_result(thread, turn).await?;
            Ok(Box::new(DroppedResult) as ResultStreamBox)
        })
    }

    fn complete_result<'a>(
        &'a self,
        thread: &'a ThreadRef,
        turn: AiTurnRef,
        result: RuntimeToolResults,
    ) -> PortFuture<'a, Result<(), br_agentic_contract::PortError>> {
        Box::pin(async move { self.inner.complete_result(thread, turn, result).await })
    }

    fn discard_result<'a>(
        &'a self,
        _thread: &'a ThreadRef,
        _entry: EntryId,
    ) -> PortFuture<'a, Result<(), br_agentic_contract::PortError>> {
        Box::pin(async { Ok(()) })
    }
}
