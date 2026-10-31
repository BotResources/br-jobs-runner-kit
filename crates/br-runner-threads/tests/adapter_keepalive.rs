mod support;

use std::sync::Arc;
use std::time::Duration;

use br_llm_messages::{ClosureReason, TurnClosure};
use br_runner_threads::test_double::{self, MemoryThreads};
use br_runner_threads::{
    Error, IgnoreReports, ItemKind, MemoryQueueStore, ReportKind, ThreadAccessAdapter,
    ThreadAccessSettings,
};
use contract_threads::{Chunk, seal_hash};
use support::adapter_wire::*;
use support::adapter_x::*;
use support::*;

const INTERVAL: Duration = Duration::from_millis(30);

fn keepalives_of(rig: &Rig, message: uuid::Uuid) -> Vec<u64> {
    chunks_of(&rig.threads, message)
        .into_iter()
        .filter(|(_, chunk)| *chunk == Chunk::Keepalive)
        .map(|(seq, _)| seq)
        .collect()
}

async fn rig() -> Rig {
    rig_keeping_alive(
        INTERVAL,
        Arc::new(MemoryQueueStore::new()),
        MemoryThreads::new(),
    )
    .await
}

async fn open_turn(
    rig: &Rig,
) -> (
    br_agentic_contract::ThreadRef,
    br_agentic_contract::AiTurnRef,
) {
    let (thread, turn) = (thread_ref(), turn_ref(rig.agent));
    rig.access()
        .add_entry(&thread, turn.clone(), notice("first"))
        .await
        .expect("accepted");
    (thread, turn)
}

async fn quiet_for_intervals() {
    tokio::time::sleep(INTERVAL * 6).await;
}

#[tokio::test]
async fn a_zero_keepalive_interval_is_refused() {
    let agent = agent_id();
    let settings = ThreadAccessSettings {
        keepalive_interval: Duration::ZERO,
        outcome_wait: Duration::from_secs(5),
        retry_wait: Duration::from_millis(20),
        store: Arc::new(MemoryQueueStore::new()),
        observer: Arc::new(IgnoreReports),
        write_context: fixed_context(context_of(agent)),
    };

    let result = ThreadAccessAdapter::new(&MemoryThreads::new().client(), settings).await;

    assert!(matches!(result, Err(Error::InvalidInput(_))));
}

#[tokio::test]
async fn an_idle_open_turn_gets_keepalives_numbered_in_order_after_its_chunks() {
    let rig = rig().await;
    let (_thread, turn) = open_turn(&rig).await;

    eventually("three keepalives", || {
        keepalives_of(&rig, turn.id.as_uuid()).len() >= 3
    })
    .await;

    assert_eq!(&keepalives_of(&rig, turn.id.as_uuid())[..3], [1, 2, 3]);
    let numbers: Vec<u64> = chunks_of(&rig.threads, turn.id.as_uuid())
        .iter()
        .map(|(seq, _)| *seq)
        .collect();
    assert_eq!(numbers, (0..numbers.len() as u64).collect::<Vec<_>>());
}

#[tokio::test]
async fn a_waiting_item_of_the_thread_holds_the_keepalives_back_until_it_ends() {
    let rig = rig().await;
    rig.threads.pause();
    let (_thread, turn) = open_turn(&rig).await;

    quiet_for_intervals().await;
    let waiting = rig.adapter.queue_state().threads[0].waiting;
    rig.threads.resume();

    assert_eq!(waiting, 2, "only the opening and the entry wait");
    eventually("a keepalive after the queue ended", || {
        !keepalives_of(&rig, turn.id.as_uuid()).is_empty()
    })
    .await;
}

#[tokio::test]
async fn an_accepted_closing_stops_the_keepalives() {
    let rig = rig().await;
    let (thread, turn) = open_turn(&rig).await;
    eventually("a keepalive", || {
        !keepalives_of(&rig, turn.id.as_uuid()).is_empty()
    })
    .await;

    rig.access()
        .close_turn(
            &thread,
            turn.clone(),
            TurnClosure::new(entry_id(), ClosureReason::cancelled()),
        )
        .await
        .expect("accepted");
    rig.idle().await;
    let sent = rig.threads.chunks().len();
    quiet_for_intervals().await;

    assert_eq!(rig.threads.chunks().len(), sent);
}

#[tokio::test]
async fn instance_ended_stops_the_keepalives_of_that_agent() {
    let rig = rig().await;
    let (_thread, turn) = open_turn(&rig).await;
    eventually("a keepalive", || {
        !keepalives_of(&rig, turn.id.as_uuid()).is_empty()
    })
    .await;

    rig.adapter.instance_ended(rig.agent).await;
    rig.idle().await;
    let sent = rig.threads.chunks().len();
    quiet_for_intervals().await;

    assert_eq!(rig.threads.chunks().len(), sent);
}

#[tokio::test]
async fn dropping_the_adapter_stops_the_keepalives() {
    let rig = rig().await;
    let (_thread, turn) = open_turn(&rig).await;
    eventually("a keepalive", || {
        !keepalives_of(&rig, turn.id.as_uuid()).is_empty()
    })
    .await;
    let Rig {
        threads, adapter, ..
    } = rig;

    drop(adapter);
    let sent = threads.operations().len();
    quiet_for_intervals().await;

    assert_eq!(threads.operations().len(), sent);
}

#[tokio::test]
async fn an_opening_refused_for_good_stops_the_keepalives() {
    let threads = MemoryThreads::new();
    threads.answer_commands(|command| {
        Err(Error::Refused(test_double::refusal(
            "thread_closed",
            "no",
            Some(command.id),
        )))
    });
    let rig = rig_keeping_alive(INTERVAL, Arc::new(MemoryQueueStore::new()), threads).await;
    open_turn(&rig).await;

    quiet_for_intervals().await;

    assert!(rig.threads.chunks().is_empty());
    let dropped_chunks = rig
        .recorder
        .reports()
        .iter()
        .filter(|report| {
            report.kind == ReportKind::ItemDroppedUnsent && report.item_kind == ItemKind::Chunk
        })
        .count();
    assert_eq!(dropped_chunks, 1, "only the entry chunk was queued");
}

#[tokio::test]
async fn the_seal_of_a_later_completion_covers_the_keepalives() {
    let rig = rig().await;
    let (thread, turn) = open_turn(&rig).await;
    eventually("two keepalives", || {
        keepalives_of(&rig, turn.id.as_uuid()).len() >= 2
    })
    .await;

    rig.access()
        .close_turn(
            &thread,
            turn.clone(),
            TurnClosure::new(entry_id(), ClosureReason::completed()),
        )
        .await
        .expect("accepted");
    rig.idle().await;

    let chunks: Vec<Chunk> = chunks_of(&rig.threads, turn.id.as_uuid())
        .into_iter()
        .map(|(_, chunk)| chunk)
        .collect();
    let br_runner_threads::test_double::CommandBody::Complete(complete) = rig
        .threads
        .commands()
        .last()
        .expect("a command")
        .body
        .clone()
    else {
        panic!("a completion");
    };
    assert_eq!(complete.seal.last_seq, chunks.len() as u64 - 1);
    assert_eq!(complete.seal.hash, seal_hash(chunks.iter()).expect("hash"));
}

#[tokio::test]
async fn a_turn_with_nothing_but_keepalives_holds_no_chunk_for_the_closing_rules() {
    let rig = rig().await;
    let (thread, turn) = (thread_ref(), turn_ref(rig.agent));
    rig.access()
        .open_result(&thread, turn.clone(), entry_id())
        .await
        .expect("accepted");
    eventually("a keepalive", || {
        !keepalives_of(&rig, turn.id.as_uuid()).is_empty()
    })
    .await;

    let completed = rig
        .access()
        .close_turn(
            &thread,
            turn.clone(),
            TurnClosure::new(entry_id(), ClosureReason::completed()),
        )
        .await;
    rig.access()
        .close_turn(
            &thread,
            turn,
            TurnClosure::new(entry_id(), ClosureReason::cancelled()),
        )
        .await
        .expect("accepted");
    rig.idle().await;

    assert!(matches!(fault_of(completed), Error::InvalidInput(_)));
    let br_runner_threads::test_double::CommandBody::Fail(fail) = rig
        .threads
        .commands()
        .last()
        .expect("a command")
        .body
        .clone()
    else {
        panic!("a failure");
    };
    assert_eq!(fail.seal, None);
}

#[tokio::test]
async fn a_keepalive_the_store_cannot_keep_is_reported_as_a_refused_chunk() {
    let store = FlakyStore::new();
    let rig = rig_keeping_alive(INTERVAL, store.clone(), MemoryThreads::new()).await;
    open_turn(&rig).await;
    rig.idle().await;

    store.refuse_appends_from_now();

    eventually("a report", || {
        rig.recorder.reports().iter().any(|report| {
            report.kind == ReportKind::ItemRefused
                && report.item_kind == ItemKind::Chunk
                && matches!(report.error, Some(Error::Store(_)))
        })
    })
    .await;
    assert!(rig.adapter.queue_state().threads.is_empty());
}
