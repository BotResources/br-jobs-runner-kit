mod support;

use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::ThreadAccess;
use br_llm_messages::{ClosureReason, TurnClosure};
use br_runner_threads::{Error, MemoryQueueStore, ReportKind, ThreadAccessAdapter, ThreadClient};
use contract_threads::{Chunk, ChunkFrame, CompleteMessage, seal_hash};
use support::nats_fixtures::*;
use support::nats_responder::{Reply, Responder};
use support::nats_server::Server;
use support::nats_streams::*;
use support::*;

struct Rig {
    _server: Server,
    bus: async_nats::Client,
    responder: Option<Responder>,
    recorder: Arc<Recorder>,
    adapter: Arc<ThreadAccessAdapter>,
    agent: br_llm_messages::AgentId,
}

async fn rig() -> Rig {
    let mut rig = rig_without_responder(Duration::from_secs(5)).await;
    rig.responder = Some(Responder::start(&rig.bus).await);
    rig
}

async fn rig_without_responder(outcome_wait: Duration) -> Rig {
    let server = Server::start();
    let bus = server.client().await;
    create_all_streams(&bus).await;
    let client: ThreadClient = built(&bus).await;
    let recorder = Arc::new(Recorder::default());
    let store = Arc::new(MemoryQueueStore::new());
    let agent = agent_id();
    let mut settings = settings(store, recorder.clone(), agent);
    settings.outcome_wait = outcome_wait;
    let adapter = ThreadAccessAdapter::new(&client, settings)
        .await
        .expect("the adapter");
    Rig {
        _server: server,
        bus,
        responder: None,
        recorder,
        adapter: Arc::new(adapter),
        agent,
    }
}

impl Rig {
    fn responder(&self) -> &Responder {
        self.responder.as_ref().expect("a responder is running")
    }

    async fn drained(&self) {
        eventually("the thread queue is empty", || {
            self.adapter
                .queue_state()
                .threads
                .iter()
                .all(|thread| thread.waiting == 0)
        })
        .await;
    }

    async fn frames(&self) -> Vec<ChunkFrame> {
        stored(&self.bus, CHUNKS)
            .await
            .iter()
            .map(|message| message.json())
            .collect()
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn an_ai_turn_reaches_the_bus_as_an_opening_then_its_chunks_then_a_sealed_closing() {
    let rig = rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);

    rig.adapter
        .add_entry(&thread, turn.clone(), notice("hello"))
        .await
        .expect("accepted");
    rig.adapter
        .complete_response(&thread, turn.clone(), response_text(entry_id(), "hi"))
        .await
        .expect("accepted");
    rig.adapter
        .close_turn(
            &thread,
            turn,
            TurnClosure::new(entry_id(), ClosureReason::completed()),
        )
        .await
        .expect("accepted");
    rig.drained().await;

    let arrivals = rig.responder().received();
    let kinds: Vec<&str> = arrivals.iter().map(|a| a.command_type()).collect();
    assert_eq!(kinds, ["message.store", "message.complete"]);
    let frames = rig.frames().await;
    assert!(frames.len() >= 2);
    assert_eq!(arrivals[0].chunks_on_arrival, 0);
    assert_eq!(arrivals[1].chunks_on_arrival, frames.len() as u64);
    let numbers: Vec<u64> = frames.iter().map(|frame| frame.seq).collect();
    assert_eq!(numbers, (0..frames.len() as u64).collect::<Vec<_>>());
    let chunks: Vec<&Chunk> = frames.iter().map(|frame| &frame.chunk).collect();
    let seal = arrivals[1].payload::<CompleteMessage>().seal;
    assert_eq!(seal.last_seq, frames.len() as u64 - 1);
    assert_eq!(seal.hash, seal_hash(chunks).expect("a hash"));
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_command_with_an_unreadable_answer_is_sent_again_identical_and_later_items_wait() {
    let rig = rig().await;
    rig.responder().script_arrivals([Reply::Unreadable]);
    let thread = thread_ref();

    rig.adapter
        .add_input_turn(&thread, input_turn(vec![human_entry("first")]))
        .await
        .expect("accepted");
    rig.adapter
        .add_input_turn(&thread, input_turn(vec![human_entry("second")]))
        .await
        .expect("accepted");
    rig.drained().await;

    let arrivals = rig.responder().received();
    let ids: Vec<_> = arrivals.iter().map(|a| a.id()).collect();
    assert_eq!(arrivals.len(), 3);
    assert_eq!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);
    assert_eq!(arrivals[0].envelope.payload, arrivals[1].envelope.payload);
    eventually("the failed attempt is reported", || {
        rig.recorder
            .reports()
            .iter()
            .any(|report| report.kind == ReportKind::AttemptFailed)
    })
    .await;
    let failed: Vec<_> = rig
        .recorder
        .reports()
        .into_iter()
        .filter(|report| report.kind == ReportKind::AttemptFailed)
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].item_id, ids[0]);
    assert!(matches!(failed[0].error, Some(Error::Unreadable(_))));
    assert_eq!(stored_count(&rig.bus, COMMANDS).await, 3);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_command_is_sent_again_while_its_stream_is_missing_and_delivered_when_it_is_back() {
    let mut rig = rig_without_responder(Duration::from_millis(300)).await;
    delete_stream(&rig.bus, COMMANDS).await;
    let thread = thread_ref();

    rig.adapter
        .add_input_turn(&thread, input_turn(vec![human_entry("waiting")]))
        .await
        .expect("accepted");

    eventually("the unavailable stream is reported", || {
        rig.adapter.queue_state().threads.iter().any(|state| {
            state.waiting == 1
                && matches!(
                    state.last_failure.as_ref().map(|failed| &failed.error),
                    Some(Error::Unavailable(_))
                )
        })
    })
    .await;
    create_stream(&rig.bus, COMMANDS).await;
    rig.responder = Some(Responder::start(&rig.bus).await);
    rig.drained().await;
    let arrivals = rig.responder().received();
    let item_id = arrivals.first().expect("the command arrived").id();
    assert!(arrivals.iter().all(|arrival| arrival.id() == item_id));
    assert_eq!(arrivals[0].command_type(), "message.store");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_chunk_the_bus_did_not_take_is_sent_again_with_the_same_number_once_the_stream_is_back() {
    let rig = rig().await;
    delete_stream(&rig.bus, CHUNKS).await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);

    rig.adapter
        .add_entry(&thread, turn, notice("kept"))
        .await
        .expect("accepted");

    eventually("the chunk failure is reported", || {
        rig.recorder.reports().iter().any(|report| {
            report.kind == ReportKind::AttemptFailed
                && matches!(report.error, Some(Error::StreamBroken))
        })
    })
    .await;
    create_stream(&rig.bus, CHUNKS).await;
    rig.drained().await;
    let frames = rig.frames().await;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].seq, 0);
    assert_eq!(rig.responder().received().len(), 1);
}
