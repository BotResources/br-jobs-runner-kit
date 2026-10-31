mod support;

#[path = "../examples/client_operations.rs"]
#[allow(dead_code)] // the example is included for its functions; its main is unused here
mod client_operations;

use br_agentic_contract::ThreadRef;
use br_llm_messages::DeltaOp;
use br_runner_threads::test_double::{CommandBody, MemoryThreads};
use br_runner_threads::{ContextContent, Error, Refusal};
use client_operations::{create_thread, read_context, stream_a_message};
use contract_threads::{Chunk, seal_hash};
use serde_json::json;
use support::adapter_wire::{
    chunks_of, context_answer, deltas_of, is_streaming, small_conversation, stores,
};
use support::*;

#[tokio::test]
async fn the_created_thread_reaches_threads_with_its_id_and_its_title() {
    let threads = MemoryThreads::new();

    let thread_id = create_thread(&threads.client()).await.expect("created");

    let commands = threads.commands();
    assert_eq!(commands.len(), 1);
    match &commands[0].body {
        CommandBody::CreateThread(creation) => {
            assert_eq!(creation.thread_id, thread_id);
            assert_eq!(creation.title.as_deref(), Some("Open tickets"));
        }
        other => panic!("not a thread creation: {other:?}"),
    }
}

#[tokio::test]
async fn a_thread_that_threads_refuses_is_an_error_of_the_client() {
    let threads = MemoryThreads::new();
    threads.answer_commands(|command| {
        Err(Error::Refused(Refusal::new(
            "KEY_REUSED",
            "the thread exists",
            Some(command.id),
        )))
    });

    let result = create_thread(&threads.client()).await;

    assert!(matches!(result, Err(Error::Refused(refusal)) if refusal.code == "KEY_REUSED"));
}

#[tokio::test]
async fn the_streamed_message_is_opened_streaming_then_closed_with_the_seal_of_its_chunks() {
    let threads = MemoryThreads::new();
    let thread_id = v7();

    let message_id = stream_a_message(&threads.client(), thread_id)
        .await
        .expect("streamed");

    let stored = stores(&threads);
    assert_eq!(stored.len(), 1);
    assert!(is_streaming(&stored[0]));
    assert_eq!(stored[0].message_id, message_id);
    assert_eq!(stored[0].thread_id, thread_id);
    let complete = threads
        .commands()
        .into_iter()
        .find_map(|command| match command.body {
            CommandBody::Complete(complete) => Some(complete),
            _ => None,
        })
        .expect("a completion");
    let chunks: Vec<Chunk> = chunks_of(&threads, message_id)
        .into_iter()
        .map(|(_, chunk)| chunk)
        .collect();
    assert_eq!(complete.message_id, message_id);
    assert_eq!(complete.seal.last_seq, chunks.len() as u64 - 1);
    assert_eq!(complete.seal.hash, seal_hash(&chunks).expect("a hash"));
}

#[tokio::test]
async fn the_chunks_are_one_entry_then_the_deltas_of_the_response_with_no_gap() {
    let threads = MemoryThreads::new();

    let message_id = stream_a_message(&threads.client(), v7())
        .await
        .expect("streamed");

    let chunks = chunks_of(&threads, message_id);
    let numbers: Vec<u64> = chunks.iter().map(|(seq, _)| *seq).collect();
    assert_eq!(numbers, (0..chunks.len() as u64).collect::<Vec<_>>());
    assert!(matches!(chunks[0].1, Chunk::Entry { .. }));
    assert!(
        chunks[1..]
            .iter()
            .all(|(_, chunk)| matches!(chunk, Chunk::Delta { .. }))
    );
}

#[tokio::test]
async fn the_response_ends_with_the_completing_delta_that_only_the_completion_sends() {
    let threads = MemoryThreads::new();

    let message_id = stream_a_message(&threads.client(), v7())
        .await
        .expect("streamed");

    let deltas = deltas_of(&threads, message_id);
    let completions = deltas
        .iter()
        .filter(|delta| matches!(delta.op(), DeltaOp::ResponseCompleted { .. }))
        .count();
    assert_eq!(completions, 1);
    assert!(matches!(
        deltas.last().expect("a delta").op(),
        DeltaOp::ResponseCompleted { .. }
    ));
}

#[tokio::test]
async fn the_context_the_service_gives_is_read_as_the_conversation_of_the_thread() {
    let threads = MemoryThreads::new();
    let thread_id = v7();
    let thread = ThreadRef::from_uuid(thread_id).expect("thread ref");
    let answer = context_answer(thread, &small_conversation(v7()));
    threads.answer_graphql(move |_, _| Ok(answer.clone()));

    let context = read_context(&threads.client(), thread_id)
        .await
        .expect("read")
        .expect("a context");

    assert_eq!(context.thread_id, thread_id);
    assert!(matches!(context.content, ContextContent::Conversation(_)));
}

#[tokio::test]
async fn a_thread_the_service_does_not_know_reads_as_no_context() {
    let threads = MemoryThreads::new();
    threads.answer_graphql(|_, _| Ok(json!({ "threadsContext": null })));

    let context = read_context(&threads.client(), v7()).await.expect("read");

    assert!(context.is_none());
}
