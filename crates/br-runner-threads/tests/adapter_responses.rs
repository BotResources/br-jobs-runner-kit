mod support;

use std::sync::Arc;

use br_llm_messages::DeltaOp;
use br_runner_threads::{Error, MemoryQueueStore};
use support::adapter_wire::*;
use support::adapter_x::*;
use support::*;

async fn plain_rig() -> Rig {
    rig_with_store(Arc::new(MemoryQueueStore::new())).await
}

fn is_completion(delta: &br_llm_messages::ResponseDelta) -> bool {
    matches!(delta.op(), DeltaOp::ResponseCompleted { .. })
}

#[tokio::test]
async fn opening_a_response_on_a_new_turn_queues_the_opening_with_the_agent_and_author_of_the_turn()
{
    let rig = plain_rig().await;
    let turn = turn_ref(agent_id());

    rig.access()
        .open_response(&thread_ref(), turn.clone(), entry_id())
        .await
        .expect("accepted");

    rig.idle().await;
    let stores = stores(&rig.threads);
    assert_eq!(stores.len(), 1);
    assert!(is_streaming(&stores[0]));
    assert_eq!(stores[0].message_id, turn.id.as_uuid());
    assert_eq!(ai_of(&stores[0]).agent_id, turn.agent.as_uuid());
    assert!(rig.threads.chunks().is_empty());
}

#[tokio::test]
async fn opening_a_response_on_an_open_turn_queues_nothing() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    rig.access()
        .add_entry(&thread, turn.clone(), notice("before"))
        .await
        .expect("accepted");
    rig.idle().await;

    rig.access()
        .open_response(&thread, turn, entry_id())
        .await
        .expect("accepted");

    assert!(rig.adapter.queue_state().threads.is_empty());
    assert_eq!(rig.threads.operations().len(), 2);
}

#[tokio::test]
async fn each_delta_of_the_sink_is_one_delta_chunk_in_order() {
    let rig = plain_rig().await;
    let turn = turn_ref(rig.agent);
    let entry = entry_id();
    let mut sink = rig
        .access()
        .open_response(&thread_ref(), turn.clone(), entry)
        .await
        .expect("accepted");
    let first = started(turn.id, entry);
    let second = inserted(turn.id, entry, block_id(), "partial");

    sink.send(first.clone()).await;
    sink.send(second.clone()).await;

    eventually("both chunks", || rig.threads.chunks().len() == 2).await;
    let numbers: Vec<u64> = chunks_of(&rig.threads, turn.id.as_uuid())
        .iter()
        .map(|(seq, _)| *seq)
        .collect();
    assert_eq!(numbers, vec![0, 1]);
    assert_eq!(
        deltas_of(&rig.threads, turn.id.as_uuid()),
        vec![first, second]
    );
}

#[tokio::test]
async fn the_completing_delta_is_not_queued_by_the_sink() {
    let rig = plain_rig().await;
    let turn = turn_ref(rig.agent);
    let entry = entry_id();
    let mut sink = rig
        .access()
        .open_response(&thread_ref(), turn.clone(), entry)
        .await
        .expect("accepted");
    sink.send(started(turn.id, entry)).await;

    sink.send(br_llm_messages::ResponseDelta::new(
        turn.id,
        entry,
        DeltaOp::response_completed(br_llm_messages::ResponseEnding::finished()),
    ))
    .await;

    rig.idle().await;
    let deltas = deltas_of(&rig.threads, turn.id.as_uuid());
    assert_eq!(deltas.len(), 1);
    assert!(!is_completion(&deltas[0]));
}

#[tokio::test]
async fn completing_an_opened_response_queues_what_the_sink_did_not_and_the_completion_last() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let (entry, block) = (entry_id(), block_id());
    let response = text_response(entry, block, "the answer");
    let mut sink = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    sink.send(started(turn.id, entry)).await;

    rig.access()
        .complete_response(&thread, turn.clone(), response.clone())
        .await
        .expect("accepted");

    eventually("three chunks", || rig.threads.chunks().len() == 3).await;
    let deltas = deltas_of(&rig.threads, turn.id.as_uuid());
    assert!(is_completion(deltas.last().expect("a delta")));
    assert_eq!(deltas.iter().filter(|d| is_completion(d)).count(), 1);
    assert_eq!(folded(&deltas, turn.id, entry), Some(response));
    assert_eq!(stores(&rig.threads).len(), 1);
}

#[tokio::test]
async fn completing_a_response_that_was_not_opened_queues_the_opening_then_its_deltas() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let entry = entry_id();
    let response = text_response(entry, block_id(), "the answer");

    rig.access()
        .complete_response(&thread, turn.clone(), response.clone())
        .await
        .expect("accepted");

    eventually("three chunks", || rig.threads.chunks().len() == 3).await;
    assert_eq!(stores(&rig.threads).len(), 1);
    assert!(is_streaming(&stores(&rig.threads)[0]));
    assert!(matches!(
        rig.threads.operations()[0],
        br_runner_threads::test_double::Operation::Command(_)
    ));
    let deltas = deltas_of(&rig.threads, turn.id.as_uuid());
    assert_eq!(folded(&deltas, turn.id, entry), Some(response));
    assert!(is_completion(deltas.last().expect("a delta")));
}

#[tokio::test]
async fn completing_a_response_that_was_not_opened_on_an_open_turn_adds_no_second_opening() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    rig.access()
        .add_entry(&thread, turn.clone(), notice("before"))
        .await
        .expect("accepted");

    rig.access()
        .complete_response(&thread, turn, text_response(entry_id(), block_id(), "x"))
        .await
        .expect("accepted");

    eventually("four chunks", || rig.threads.chunks().len() == 4).await;
    assert_eq!(stores(&rig.threads).len(), 1);
}

#[tokio::test]
async fn a_sink_whose_response_was_completed_takes_what_it_is_sent_and_does_nothing() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let (entry, block) = (entry_id(), block_id());
    let mut sink = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    rig.access()
        .complete_response(&thread, turn.clone(), text_response(entry, block, "done"))
        .await
        .expect("accepted");
    rig.idle().await;
    let before = rig.threads.chunks().len();

    sink.send(inserted(turn.id, entry, block_id(), "late"))
        .await;

    rig.idle().await;
    assert_eq!(rig.threads.chunks().len(), before);
}

#[tokio::test]
async fn completing_a_response_that_the_queued_deltas_cannot_reach_is_refused_and_queues_nothing() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let entry = entry_id();
    let mut sink = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    sink.send(started(turn.id, entry)).await;
    sink.send(inserted(turn.id, entry, block_id(), "one block"))
        .await;
    rig.idle().await;
    let before = rig.threads.chunks().len();

    let result = rig
        .access()
        .complete_response(
            &thread,
            turn,
            text_response(entry, block_id(), "another block"),
        )
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    rig.idle().await;
    assert_eq!(rig.threads.chunks().len(), before);
}
