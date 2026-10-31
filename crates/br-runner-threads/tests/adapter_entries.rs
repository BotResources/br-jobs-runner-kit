mod support;

use std::sync::Arc;

use br_agentic_contract::AiTurnRef;
use br_llm_messages::TurnEntry;
use br_runner_threads::test_double::Operation;
use br_runner_threads::{Error, MemoryQueueStore, QueueStore};
use futures::future::join_all;
use support::adapter_wire::*;
use support::adapter_x::*;
use support::*;

fn turn_by(agent: br_llm_messages::AgentId, name: &str) -> AiTurnRef {
    AiTurnRef {
        id: turn_id(),
        agent,
        author: author(name),
    }
}

#[tokio::test]
async fn the_first_entry_of_a_turn_queues_the_opening_with_the_agent_and_author_of_the_turn() {
    let rig = rig_with_store(Arc::new(MemoryQueueStore::new())).await;
    let thread = thread_ref();
    let other_agent = agent_id();
    let turn = turn_by(other_agent, "Turn Author");
    let entry = notice("hello");

    rig.access()
        .add_entry(&thread, turn.clone(), entry.clone())
        .await
        .expect("accepted");

    rig.idle().await;
    let stores = stores(&rig.threads);
    assert_eq!(stores.len(), 1);
    assert!(is_streaming(&stores[0]));
    assert_eq!(stores[0].message_id, turn.id.as_uuid());
    assert_eq!(stores[0].thread_id, thread.as_uuid());
    assert_eq!(ai_of(&stores[0]).agent_id, other_agent.as_uuid());
    assert_ne!(other_agent, rig.agent);
    assert_eq!(ai_of(&stores[0]).author_name, "Turn Author");
    let chunks = chunks_of(&rig.threads, turn.id.as_uuid());
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].0, 0);
    assert_eq!(entry_of(&chunks[0].1), entry);
    assert!(matches!(rig.threads.operations()[0], Operation::Command(_)));
    assert!(matches!(
        rig.threads.operations()[1],
        Operation::Chunk { .. }
    ));
}

#[tokio::test]
async fn a_later_entry_of_the_same_turn_is_one_more_chunk_and_no_second_opening() {
    let w = world().await;
    let thread = thread_ref();
    let turn = turn_ref(w.agent);
    let first = notice("one");
    let second = task_started();

    w.access()
        .add_entry(&thread, turn.clone(), first.clone())
        .await
        .expect("accepted");
    w.access()
        .add_entry(&thread, turn.clone(), second.clone())
        .await
        .expect("accepted");

    eventually("both chunks delivered", || w.threads.chunks().len() == 2).await;
    assert_eq!(stores(&w.threads).len(), 1);
    let chunks = chunks_of(&w.threads, turn.id.as_uuid());
    assert_eq!(chunks[0].0, 0);
    assert_eq!(chunks[1].0, 1);
    assert_eq!(entry_of(&chunks[0].1), first);
    assert_eq!(entry_of(&chunks[1].1), second);
}

#[tokio::test]
async fn entries_added_at_once_to_a_new_turn_queue_one_opening_and_contiguous_numbers() {
    let w = world().await;
    let thread = thread_ref();
    let turn = turn_ref(w.agent);

    let additions = (0..6).map(|n| {
        w.access()
            .add_entry(&thread, turn.clone(), notice(&format!("entry {n}")))
    });
    for result in join_all(additions).await {
        result.expect("accepted");
    }

    eventually("all delivered", || w.threads.chunks().len() == 6).await;
    assert_eq!(stores(&w.threads).len(), 1);
    let numbers: Vec<u64> = chunks_of(&w.threads, turn.id.as_uuid())
        .iter()
        .map(|(seq, _)| *seq)
        .collect();
    assert_eq!(numbers, (0..6).collect::<Vec<u64>>());
    assert!(matches!(w.threads.operations()[0], Operation::Command(_)));
}

#[tokio::test]
async fn an_entry_that_is_a_model_response_or_a_closure_is_refused_before_queueing() {
    let store = Arc::new(MemoryQueueStore::new());
    let rig = rig_over(
        store.clone(),
        fixed_context(context_of(agent_id())),
        agent_id(),
    )
    .await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let response = TurnEntry::from(response_text(entry_id(), "hi"));
    let closure = TurnEntry::from(br_llm_messages::TurnClosure::new(
        entry_id(),
        br_llm_messages::ClosureReason::completed(),
    ));

    let for_response = rig
        .access()
        .add_entry(&thread, turn.clone(), response)
        .await;
    let for_closure = rig.access().add_entry(&thread, turn.clone(), closure).await;

    assert!(matches!(fault_of(for_response), Error::InvalidInput(_)));
    assert!(matches!(fault_of(for_closure), Error::InvalidInput(_)));
    assert!(store.list().await.expect("list").is_empty());
    assert!(rig.adapter.queue_state().threads.is_empty());
    assert!(rig.threads.operations().is_empty());
}

#[tokio::test]
async fn an_entry_is_refused_before_queueing_when_the_write_context_fails() {
    let store = Arc::new(MemoryQueueStore::new());
    let rig = rig_over(store.clone(), failing_context("no writer"), agent_id()).await;

    let result = rig
        .access()
        .add_entry(&thread_ref(), turn_ref(rig.agent), notice("hello"))
        .await;

    assert!(result.expect_err("refused").is_refusal());
    assert!(store.list().await.expect("list").is_empty());
    assert!(rig.threads.operations().is_empty());
}

#[tokio::test]
async fn a_tool_result_is_one_entry_chunk_after_the_opening_of_a_new_turn() {
    let w = world().await;
    let thread = thread_ref();
    let turn = turn_ref(w.agent);
    let call = block_id();
    let results = tool_results(entry_id(), call, "search", "found");
    let entry = TurnEntry::from(results.clone());

    w.access()
        .complete_result(&thread, turn.clone(), results)
        .await
        .expect("accepted");

    eventually("delivered", || w.threads.chunks().len() == 1).await;
    assert_eq!(stores(&w.threads).len(), 1);
    assert!(is_streaming(&stores(&w.threads)[0]));
    let chunks = chunks_of(&w.threads, turn.id.as_uuid());
    assert_eq!(chunks[0].0, 0);
    assert_eq!(entry_of(&chunks[0].1), entry);
}

#[tokio::test]
async fn a_tool_result_for_an_open_turn_is_only_a_chunk() {
    let w = world().await;
    let thread = thread_ref();
    let turn = turn_ref(w.agent);
    w.access()
        .add_entry(&thread, turn.clone(), notice("before"))
        .await
        .expect("accepted");

    w.access()
        .complete_result(
            &thread,
            turn.clone(),
            tool_results(entry_id(), block_id(), "search", "found"),
        )
        .await
        .expect("accepted");

    eventually("both delivered", || w.threads.chunks().len() == 2).await;
    assert_eq!(stores(&w.threads).len(), 1);
    assert_eq!(chunks_of(&w.threads, turn.id.as_uuid())[1].0, 1);
}

#[tokio::test]
async fn chunk_numbers_follow_one_another_across_entries_responses_and_results() {
    let w = world().await;
    let thread = thread_ref();
    let turn = turn_ref(w.agent);
    let entry = entry_id();
    let block = block_id();
    let ops = w.access();

    ops.add_entry(&thread, turn.clone(), notice("a"))
        .await
        .expect("accepted");
    ops.complete_response(&thread, turn.clone(), text_response(entry, block, "answer"))
        .await
        .expect("accepted");
    ops.complete_result(
        &thread,
        turn.clone(),
        tool_results(entry_id(), block_id(), "search", "found"),
    )
    .await
    .expect("accepted");
    ops.add_entry(&thread, turn.clone(), notice("b"))
        .await
        .expect("accepted");

    eventually("all delivered", || w.threads.chunks().len() == 6).await;
    let numbers: Vec<u64> = chunks_of(&w.threads, turn.id.as_uuid())
        .iter()
        .map(|(seq, _)| *seq)
        .collect();
    assert_eq!(numbers, (0..6).collect::<Vec<u64>>());
    assert_eq!(stores(&w.threads).len(), 1);
}

#[tokio::test]
async fn a_failed_append_refuses_with_store_and_leaves_no_gap_in_the_numbers() {
    let flaky = FlakyStore::new();
    let rig = rig_with_store(flaky.clone()).await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    rig.access()
        .add_entry(&thread, turn.clone(), notice("first"))
        .await
        .expect("accepted");

    flaky.accept_only(2);
    let refused = rig
        .access()
        .complete_response(
            &thread,
            turn.clone(),
            text_response(entry_id(), block_id(), "answer"),
        )
        .await;
    flaky.accept_appends_again();
    rig.access()
        .add_entry(&thread, turn.clone(), notice("after"))
        .await
        .expect("accepted");

    assert!(matches!(fault_of(refused), Error::Store(_)));
    eventually("kept chunks delivered", || rig.threads.chunks().len() == 4).await;
    let numbers: Vec<u64> = chunks_of(&rig.threads, turn.id.as_uuid())
        .iter()
        .map(|(seq, _)| *seq)
        .collect();
    assert_eq!(numbers, vec![0, 1, 2, 3]);
}

#[tokio::test]
async fn an_opening_whose_append_fails_is_refused_and_leaves_the_turn_unopened() {
    let flaky = FlakyStore::new();
    let rig = rig_with_store(flaky.clone()).await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    flaky.refuse_appends_from_now();

    let refused = rig
        .access()
        .add_entry(&thread, turn.clone(), notice("first"))
        .await;
    flaky.accept_appends_again();
    rig.access()
        .add_entry(&thread, turn.clone(), notice("again"))
        .await
        .expect("accepted");

    assert!(matches!(fault_of(refused), Error::Store(_)));
    rig.idle().await;
    assert_eq!(stores(&rig.threads).len(), 1);
    assert_eq!(chunks_of(&rig.threads, turn.id.as_uuid())[0].0, 0);
}
