mod support;

use br_runner_threads::test_double::{CommandBody, MemoryThreads, Outcome};
use br_runner_threads::{ClosureKind, Error, MessageStatus};
use support::client_closing_x::*;
use support::client_x::*;
use support::*;

#[tokio::test]
async fn a_confirmed_closing_spends_the_handle_its_writers_and_its_sinks() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 1).await;
    let writer = handle.writer();
    let response_entry = entry_id();
    let (mut sink, control) = handle.response(response_entry).expect("a pair");
    threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Completed), Some(&handle))
        .await
        .expect("confirmed");
    let sent = threads.chunks().len();

    let by_handle = handle.keepalive().await;
    let by_writer = writer.entry(&notice("late")).await;
    let delta = handle.delta(&started(open.message_id, entry_id())).await;
    br_agentic_contract::StreamSink::send(&mut sink, started(open.message_id, response_entry))
        .await;

    assert_eq!(by_handle, Err(Error::StreamClosed));
    assert_eq!(by_writer, Err(Error::StreamClosed));
    assert_eq!(delta, Err(Error::StreamClosed));
    assert_eq!(control.failure(), Some(Error::StreamClosed));
    assert_eq!(threads.chunks().len(), sent);
}

#[tokio::test]
async fn a_chunk_is_judged_on_the_state_of_the_handle_before_its_content() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 1).await;
    threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Completed), Some(&handle))
        .await
        .expect("confirmed");

    let foreign = handle.delta(&started(turn_id(), entry_id())).await;

    assert_eq!(foreign, Err(Error::StreamClosed));
}

#[tokio::test]
async fn a_second_closing_of_a_spent_handle_is_refused_as_closed_and_sends_nothing() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 1).await;
    let client = threads.client();
    client
        .close_message(&closing_of(&open, ClosureKind::Completed), Some(&handle))
        .await
        .expect("confirmed");
    let sent = threads.operations().len();

    for kind in [ClosureKind::Completed, ClosureKind::Cancelled] {
        let again = client
            .close_message(&closing_of(&open, kind), Some(&handle))
            .await;
        assert_eq!(again, Err(Error::StreamClosed), "{kind:?}");
    }
    assert_eq!(threads.operations().len(), sent);
}

#[tokio::test]
async fn a_closing_that_confirms_failed_is_still_a_confirmation_and_spends_the_handle() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 1).await;
    threads.answer_commands(|command| {
        Ok(Outcome {
            command_id: command.id,
            status: Some(MessageStatus::Failed),
        })
    });

    let confirmed = threads
        .client()
        .close_message(&closing_of(&open, ClosureKind::Completed), Some(&handle))
        .await
        .expect("a confirmation");

    assert_eq!(confirmed.status, Some(MessageStatus::Failed));
    assert_eq!(handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
async fn a_closing_goes_after_every_chunk_sent_before_it() {
    let threads = MemoryThreads::new();
    let (handle, open) = opened(&threads.client()).await;
    threads.pause();
    let writer = handle.writer();
    let chunk_task = tokio::spawn(async move { writer.entry(&notice("filler")).await });
    eventually("the chunk reached the bus", || threads.chunks().len() == 1).await;
    let client = threads.client();
    let closing = closing_of(&open, ClosureKind::Completed);
    let closing_task = tokio::spawn(async move {
        let handle = handle;
        client.close_message(&closing, Some(&handle)).await
    });

    settle().await;
    let commands_while_chunk_in_flight = threads.commands().len();
    threads.resume();
    chunk_task.await.expect("joined").expect("taken");
    closing_task.await.expect("joined").expect("confirmed");

    assert_eq!(commands_while_chunk_in_flight, 1);
    let CommandBody::Complete(complete) = last_command(&threads).body else {
        panic!("not a completion");
    };
    assert_eq!(complete.seal.last_seq, 0);
    assert_eq!(complete.seal, seal_of(&threads));
}

#[tokio::test]
async fn while_a_closing_waits_the_handle_takes_no_chunk() {
    let threads = MemoryThreads::new();
    let (handle, open) = with_chunks(&threads, 1).await;
    let before = threads.commands().len();
    threads.pause();
    let client = threads.client();
    let closing = closing_of(&open, ClosureKind::Cancelled);
    let handle = std::sync::Arc::new(handle);
    let shared = std::sync::Arc::clone(&handle);
    let task = tokio::spawn(async move { client.close_message(&closing, Some(&shared)).await });
    eventually("the closing reached Threads", || {
        threads.commands().len() == before + 1
    })
    .await;

    let while_waiting = handle.keepalive().await;
    threads.resume();
    task.await.expect("joined").expect("confirmed");

    assert_eq!(while_waiting, Err(Error::StreamClosed));
    assert_eq!(threads.chunks().len(), 1);
}
