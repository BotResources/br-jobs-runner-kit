mod support;

use std::sync::Arc;

use br_agentic_contract::{AiTurnRef, ThreadRef};
use br_llm_messages::{ClosureReason, TurnClosure};
use br_runner_threads::{Error, MemoryQueueStore};
use support::adapter_wire::*;
use support::adapter_x::*;
use support::*;

fn closing(reason: ClosureReason) -> TurnClosure {
    TurnClosure::new(entry_id(), reason)
}

async fn closed_turn(rig: &Rig) -> (ThreadRef, AiTurnRef) {
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    rig.access()
        .add_entry(&thread, turn.clone(), notice("work"))
        .await
        .expect("accepted");
    rig.access()
        .close_turn(&thread, turn.clone(), closing(ClosureReason::completed()))
        .await
        .expect("accepted");
    rig.idle().await;
    (thread, turn)
}

async fn plain_rig() -> Rig {
    rig_with_store(Arc::new(MemoryQueueStore::new())).await
}

fn assert_nothing_more_was_sent(rig: &Rig, operations_before: usize) {
    assert!(rig.adapter.queue_state().threads.is_empty());
    assert_eq!(rig.threads.operations().len(), operations_before);
}

#[tokio::test]
async fn an_entry_for_a_turn_whose_closing_was_accepted_is_refused() {
    let rig = plain_rig().await;
    let (thread, turn) = closed_turn(&rig).await;
    let before = rig.threads.operations().len();

    let result = rig.access().add_entry(&thread, turn, notice("late")).await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    assert_nothing_more_was_sent(&rig, before);
}

#[tokio::test]
async fn a_tool_result_for_a_turn_whose_closing_was_accepted_is_refused() {
    let rig = plain_rig().await;
    let (thread, turn) = closed_turn(&rig).await;
    let before = rig.threads.operations().len();

    let result = rig
        .access()
        .complete_result(
            &thread,
            turn,
            tool_results(entry_id(), block_id(), "search", "late"),
        )
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    assert_nothing_more_was_sent(&rig, before);
}

#[tokio::test]
async fn opening_a_response_for_a_turn_whose_closing_was_accepted_is_refused() {
    let rig = plain_rig().await;
    let (thread, turn) = closed_turn(&rig).await;
    let before = rig.threads.operations().len();

    let result = rig.access().open_response(&thread, turn, entry_id()).await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    assert_nothing_more_was_sent(&rig, before);
}

#[tokio::test]
async fn opening_a_result_for_a_turn_whose_closing_was_accepted_is_refused() {
    let rig = plain_rig().await;
    let (thread, turn) = closed_turn(&rig).await;
    let before = rig.threads.operations().len();

    let result = rig.access().open_result(&thread, turn, entry_id()).await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    assert_nothing_more_was_sent(&rig, before);
}

#[tokio::test]
async fn completing_a_response_for_a_turn_whose_closing_was_accepted_is_refused() {
    let rig = plain_rig().await;
    let (thread, turn) = closed_turn(&rig).await;
    let before = rig.threads.operations().len();

    let result = rig
        .access()
        .complete_response(&thread, turn, text_response(entry_id(), block_id(), "late"))
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    assert_nothing_more_was_sent(&rig, before);
}

#[tokio::test]
async fn a_second_closing_for_the_same_turn_is_refused() {
    let rig = plain_rig().await;
    let (thread, turn) = closed_turn(&rig).await;
    let before = rig.threads.operations().len();

    let result = rig
        .access()
        .close_turn(&thread, turn, closing(ClosureReason::cancelled()))
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    assert_nothing_more_was_sent(&rig, before);
}

#[tokio::test]
async fn discarding_a_response_or_a_result_after_the_closing_is_accepted_and_changes_nothing() {
    let rig = plain_rig().await;
    let (thread, _turn) = closed_turn(&rig).await;
    let before = rig.threads.operations().len();

    rig.access()
        .discard_response(&thread, entry_id())
        .await
        .expect("accepted");
    rig.access()
        .discard_result(&thread, entry_id())
        .await
        .expect("accepted");

    assert_nothing_more_was_sent(&rig, before);
}

#[tokio::test]
async fn the_turn_is_closed_when_the_closing_is_accepted_even_if_threads_has_not_taken_it() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    rig.access()
        .add_entry(&thread, turn.clone(), notice("work"))
        .await
        .expect("accepted");
    rig.idle().await;
    rig.threads.pause();
    rig.access()
        .close_turn(&thread, turn.clone(), closing(ClosureReason::completed()))
        .await
        .expect("accepted");

    let late = rig.access().add_entry(&thread, turn, notice("late")).await;

    assert!(matches!(fault_of(late), Error::InvalidInput(_)));
    assert_eq!(rig.adapter.queue_state().threads[0].waiting, 1);
}

#[tokio::test]
async fn a_closing_for_a_turn_the_adapter_never_saw_closes_it_for_later_operations() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    rig.access()
        .close_turn(&thread, turn.clone(), closing(ClosureReason::cancelled()))
        .await
        .expect("accepted");
    eventually("the closing is delivered", || {
        rig.threads.commands().len() == 1
    })
    .await;
    rig.idle().await;
    let before = rig.threads.operations().len();

    let result = rig.access().add_entry(&thread, turn, notice("late")).await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    assert_nothing_more_was_sent(&rig, before);
}

#[tokio::test]
async fn the_closing_of_one_turn_leaves_another_turn_of_the_thread_open() {
    let rig = plain_rig().await;
    let (thread, _closed) = closed_turn(&rig).await;
    let other = turn_ref(rig.agent);

    rig.access()
        .add_entry(&thread, other.clone(), notice("still open"))
        .await
        .expect("accepted");

    rig.idle().await;
    assert_eq!(chunks_of(&rig.threads, other.id.as_uuid()).len(), 1);
}
