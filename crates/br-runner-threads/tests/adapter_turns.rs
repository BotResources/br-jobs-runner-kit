mod support;

use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::AiTurnRef;
use br_llm_messages::Turn;
use br_runner_threads::{Error, MemoryQueueStore, QueueStore, WriteContext};
use contract_threads::AiDelivery;
use support::adapter_wire::*;
use support::adapter_x::*;
use support::*;

#[tokio::test]
async fn an_input_turn_goes_to_threads_as_one_whole_message_under_the_write_context() {
    let agent = agent_id();
    let (job, user, impersonator) = (v7(), v7(), v7());
    let rig = rig_with_context(WriteContext {
        agent,
        author: author("Writer"),
        job_id: Some(job),
        acting_user_id: Some(user),
        impersonator_id: Some(impersonator),
    })
    .await;
    let thread = thread_ref();
    let turn = input_turn(vec![notice_entry("first"), notice_entry("second")]);

    rig.access()
        .add_input_turn(&thread, turn.clone())
        .await
        .expect("accepted");

    rig.idle().await;
    let stores = stores(&rig.threads);
    assert_eq!(stores.len(), 1);
    assert!(rig.threads.chunks().is_empty());
    let store = &stores[0];
    assert_eq!(store.message_id, turn.id().as_uuid());
    assert_eq!(store.thread_id, thread.as_uuid());
    assert_eq!(store.parent_message_id, None);
    assert_eq!(store.job_id, Some(job));
    assert_eq!(store.acting_user_id, Some(user));
    assert_eq!(store.impersonator_id, Some(impersonator));
    let ai = ai_of(store);
    assert_eq!(ai.agent_id, agent.as_uuid());
    assert_eq!(ai.author_name, "Writer");
    let AiDelivery::Completed { turn: body, .. } = &ai.delivery else {
        panic!("an input turn is a completed message");
    };
    assert_eq!(body.to_turn().expect("a turn"), Turn::InputTurn(turn));
}

#[tokio::test]
async fn an_input_turn_keeps_no_turn_state_so_a_completed_closing_for_it_is_refused() {
    let w = world().await;
    let thread = thread_ref();
    let turn = input_turn(vec![notice_entry("work")]);
    let id = turn.id();
    w.access()
        .add_input_turn(&thread, turn)
        .await
        .expect("accepted");

    let closing =
        br_llm_messages::TurnClosure::new(entry_id(), br_llm_messages::ClosureReason::completed());
    let result = w
        .access()
        .close_turn(
            &thread,
            AiTurnRef {
                id,
                agent: w.agent,
                author: author("Analyst"),
            },
            closing,
        )
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
}

#[tokio::test]
async fn an_input_turn_is_refused_before_queueing_when_the_write_context_fails() {
    let store = Arc::new(MemoryQueueStore::new());
    let rig = rig_over(store.clone(), failing_context("no writer"), agent_id()).await;

    let result = rig
        .access()
        .add_input_turn(&thread_ref(), input_turn(vec![notice_entry("work")]))
        .await;

    assert!(result.expect_err("refused").is_refusal());
    assert!(store.list().await.expect("list").is_empty());
    assert!(rig.adapter.queue_state().threads.is_empty());
    assert!(rig.threads.operations().is_empty());
}

#[tokio::test]
async fn the_entries_of_a_stored_input_turn_count_as_stored() {
    let w = world().await;
    let thread = thread_ref();
    let entry = notice_entry("work");
    let wanted = entry.id();
    w.threads.pause();
    w.access()
        .add_input_turn(&thread, input_turn(vec![entry]))
        .await
        .expect("accepted");

    let pending = tokio::time::timeout(
        Duration::from_millis(150),
        w.adapter.entry_stored(&thread, wanted),
    )
    .await;
    w.threads.resume();

    assert!(pending.is_err());
    assert_eq!(
        w.adapter.entry_stored(&thread, wanted).await,
        br_runner_threads::EntryOutcome::Stored
    );
}

#[tokio::test]
async fn the_entries_of_an_input_turn_that_threads_refused_count_as_dropped() {
    let w = world().await;
    w.threads
        .answer_commands(|_command| Err(Error::Unauthenticated));
    let thread = thread_ref();
    let entry = notice_entry("work");
    let wanted = entry.id();

    w.access()
        .add_input_turn(&thread, input_turn(vec![entry]))
        .await
        .expect("accepted");

    assert_eq!(
        w.adapter.entry_stored(&thread, wanted).await,
        br_runner_threads::EntryOutcome::Dropped
    );
}
