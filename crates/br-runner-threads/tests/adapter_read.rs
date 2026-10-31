mod support;

use br_runner_threads::test_double;
use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::ThreadRef;
use br_llm_messages::Conversation;
use br_runner_threads::test_double::{CommandBody, MemoryThreads, Operation, Outcome};
use br_runner_threads::{Cause, Error, MemoryQueueStore, MessageStatus};
use serde_json::json;
use support::adapter_wire::*;
use support::adapter_x::*;
use support::*;

fn answer_with(threads: &MemoryThreads, thread: ThreadRef, conversation: Conversation) {
    threads.answer_graphql(move |_document, _variables| Ok(context_answer(thread, &conversation)));
}

fn graphql_count(threads: &MemoryThreads) -> usize {
    threads
        .operations()
        .iter()
        .filter(|op| matches!(op, Operation::Graphql { .. }))
        .count()
}

fn stored_outcome(command_id: uuid::Uuid) -> Outcome {
    Outcome {
        command_id,
        status: Some(MessageStatus::Completed),
    }
}

#[tokio::test]
async fn a_read_asks_threads_for_the_context_of_the_thread_with_no_agent_and_gives_its_turns() {
    let w = world().await;
    let thread = thread_ref();
    let held = small_conversation(thread.as_uuid());
    answer_with(&w.threads, thread, held.clone());

    let conversation = w.access().read(&thread).await.expect("a conversation");

    assert_eq!(conversation, held);
    let Operation::Graphql { variables, .. } = &w.threads.operations()[0] else {
        panic!("a query");
    };
    assert_eq!(variables["threadId"], json!(thread.as_uuid()));
    assert_eq!(variables["agentId"], json!(null));
    assert!(w.threads.commands().is_empty());
}

#[tokio::test]
async fn a_read_gives_the_thread_id_as_the_identity_of_the_conversation() {
    let w = world().await;
    let thread = thread_ref();
    let held = small_conversation(v7());
    assert_ne!(held.id().as_uuid(), thread.as_uuid());
    answer_with(&w.threads, thread, held.clone());

    let conversation = w.access().read(&thread).await.expect("a conversation");

    assert_eq!(conversation.id().as_uuid(), thread.as_uuid());
    assert_eq!(conversation.turns(), held.turns());
}

#[tokio::test]
async fn a_read_waits_until_the_items_accepted_before_it_have_ended() {
    let w = world().await;
    let thread = thread_ref();
    answer_with(&w.threads, thread, small_conversation(thread.as_uuid()));
    w.threads.pause();
    w.access()
        .add_input_turn(&thread, input_turn(vec![notice_entry("work")]))
        .await
        .expect("accepted");

    let reader = w.access().read(&thread);
    let releaser = async {
        settle().await;
        let asked_before_release = graphql_count(&w.threads);
        w.threads.resume();
        asked_before_release
    };
    let (conversation, asked_before_release) = tokio::join!(reader, releaser);

    conversation.expect("a conversation");
    assert_eq!(asked_before_release, 0);
    let operations = w.threads.operations();
    assert!(matches!(operations[0], Operation::Command(_)));
    assert!(matches!(operations[1], Operation::Graphql { .. }));
}

#[tokio::test]
async fn a_read_waits_for_the_leftovers_of_the_store_and_they_go_first() {
    let store = Arc::new(MemoryQueueStore::new());
    let thread = thread_ref();
    let first = rig_over(
        store.clone(),
        fixed_context(context_of(agent_id())),
        agent_id(),
    )
    .await;
    first.threads.pause();
    first
        .access()
        .add_input_turn(&thread, input_turn(vec![notice_entry("work")]))
        .await
        .expect("accepted");
    drop(first);
    let threads = MemoryThreads::new();
    threads.pause();
    answer_with(&threads, thread, small_conversation(thread.as_uuid()));
    let second = rig_on(
        threads.clone(),
        store,
        fixed_context(context_of(agent_id())),
        agent_id(),
    )
    .await;

    let reader = second.access().read(&thread);
    let releaser = async {
        settle().await;
        let asked_before_release = graphql_count(&threads);
        threads.resume();
        asked_before_release
    };
    let (conversation, asked_before_release) = tokio::join!(reader, releaser);

    conversation.expect("a conversation");
    assert_eq!(asked_before_release, 0);
    assert_eq!(stores(&threads).len(), 1);
    assert!(matches!(threads.operations()[0], Operation::Command(_)));
}

#[tokio::test]
async fn a_read_does_not_wait_for_an_item_that_ended_refused() {
    let w = world().await;
    let thread = thread_ref();
    answer_with(&w.threads, thread, small_conversation(thread.as_uuid()));
    w.threads
        .answer_commands(|_command| Err(Error::Unauthenticated));
    w.access()
        .add_input_turn(&thread, input_turn(vec![notice_entry("work")]))
        .await
        .expect("accepted");

    let read = tokio::time::timeout(Duration::from_secs(3), w.access().read(&thread)).await;

    read.expect("the read did not hang")
        .expect("a conversation");
}

#[tokio::test]
async fn a_read_of_one_thread_does_not_wait_for_the_blocked_items_of_another_thread() {
    let w = world().await;
    let (blocked, free) = (thread_ref(), thread_ref());
    let blocked_id = blocked.as_uuid();
    w.threads
        .answer_commands(move |command| match &command.body {
            CommandBody::Store(store) if store.thread_id == blocked_id => Err(Error::Unavailable(
                test_double::failure(Cause::Transport, "threads is down for this thread"),
            )),
            _ => Ok(stored_outcome(command.id)),
        });
    answer_with(&w.threads, free, small_conversation(free.as_uuid()));
    w.access()
        .add_input_turn(&blocked, input_turn(vec![notice_entry("stuck")]))
        .await
        .expect("accepted");

    let free_read = tokio::time::timeout(Duration::from_secs(3), w.access().read(&free)).await;
    let blocked_read =
        tokio::time::timeout(Duration::from_millis(200), w.access().read(&blocked)).await;

    free_read.expect("not blocked").expect("a conversation");
    assert!(blocked_read.is_err());
}

#[tokio::test]
async fn a_thread_that_does_not_exist_is_a_fault_with_invalid_input_as_cause() {
    let w = world().await;
    w.threads
        .answer_graphql(|_document, _variables| Ok(json!({ "threadsContext": null })));

    let result = w.access().read(&thread_ref()).await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
}

#[tokio::test]
async fn a_failure_of_threads_is_a_fault_with_that_error_as_cause_and_is_not_repeated() {
    let w = world().await;
    let failure = Error::Unavailable(test_double::failure(Cause::Transport, "no gateway"));
    let answer = failure.clone();
    w.threads
        .answer_graphql(move |_document, _variables| Err(answer.clone()));

    let result = w.access().read(&thread_ref()).await;

    assert_eq!(fault_of(result), failure);
    assert_eq!(graphql_count(&w.threads), 1);
}

#[tokio::test]
async fn a_context_without_a_conversation_is_a_fault_with_unreadable_as_cause() {
    let w = world().await;
    let thread = thread_ref();
    w.threads.answer_graphql(move |_document, _variables| {
        Ok(json!({ "threadsContext": {
            "threadId": thread.as_uuid(),
            "conversation": null,
            "projectedThread": {},
            "members": [],
            "resources": [],
        }}))
    });

    let result = w.access().read(&thread).await;

    assert!(matches!(fault_of(result), Error::Unreadable(_)));
}

#[tokio::test]
async fn a_conversation_document_that_the_library_refuses_is_a_fault_with_unreadable_as_cause() {
    let w = world().await;
    let thread = thread_ref();
    w.threads.answer_graphql(move |_document, _variables| {
        Ok(json!({ "threadsContext": {
            "threadId": thread.as_uuid(),
            "conversation": { "not": "a conversation" },
            "projectedThread": null,
            "members": [],
            "resources": [],
        }}))
    });

    let result = w.access().read(&thread).await;

    assert!(matches!(fault_of(result), Error::Unreadable(_)));
}

#[tokio::test]
async fn an_answer_without_the_context_field_is_a_fault_with_unreadable_as_cause() {
    let w = world().await;
    w.threads
        .answer_graphql(|_document, _variables| Ok(json!({})));

    let result = w.access().read(&thread_ref()).await;

    assert!(matches!(fault_of(result), Error::Unreadable(_)));
}
