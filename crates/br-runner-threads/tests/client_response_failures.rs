mod support;

use br_llm_messages::{DeltaOp, ResponseMetadata};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{ClosureKind, Error};
use support::client_streaming::*;
use support::client_x::*;
use support::*;

#[tokio::test]
async fn a_kept_invalid_input_does_not_stop_a_completion() {
    let mut s = streaming().await;
    s.send(DeltaOp::text_appended(block_id(), "refused")).await;
    assert!(matches!(s.control.failure(), Some(Error::InvalidInput(_))));
    let response = text_response(s.entry, &[(block_id(), "whole")]);

    s.control.complete(&response).await.expect("completes");

    assert_eq!(s.folded(), Some(response));
}

#[tokio::test]
async fn a_kept_stream_broken_refuses_a_completion_and_sends_nothing() {
    let mut s = streaming().await;
    s.threads
        .answer_chunks(|_, _, _| Err(Error::Unauthenticated));
    s.send(DeltaOp::response_started(ResponseMetadata::new()))
        .await;
    s.threads.answer_chunks(|_, _, _| Ok(()));
    let sent = s.threads.chunks().len();

    let result = s
        .control
        .complete(&text_response(s.entry, &[(block_id(), "x")]))
        .await;

    assert_eq!(result, Err(Error::StreamBroken));
    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn a_kept_stream_closed_refuses_a_completion_and_sends_nothing() {
    let mut s = streaming().await;
    s.handle.keepalive().await.expect("taken");
    s.threads
        .client()
        .close_message(
            &closing_of(&s.open, ClosureKind::Cancelled),
            Some(&s.handle),
        )
        .await
        .expect("confirmed");
    s.send(DeltaOp::response_started(ResponseMetadata::new()))
        .await;
    let sent = s.threads.chunks().len();

    let result = s
        .control
        .complete(&text_response(s.entry, &[(block_id(), "x")]))
        .await;

    assert_eq!(result, Err(Error::StreamClosed));
    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn a_completion_whose_chunk_the_bus_refuses_midway_fails_and_is_kept() {
    let s = streaming().await;
    s.threads.answer_chunks(|_, seq, _| {
        if seq < 1 {
            Ok(())
        } else {
            Err(Error::Unauthenticated)
        }
    });
    let response = text_response(s.entry, &[(block_id(), "x")]);

    let result = s.control.complete(&response).await;
    let again = s.control.complete(&response).await;

    assert_eq!(result, Err(Error::StreamBroken));
    assert_eq!(again, Err(Error::StreamBroken));
    assert_eq!(s.control.failure(), Some(Error::StreamBroken));
}

#[tokio::test]
async fn a_completion_on_the_response_of_a_writer_reaches_the_same_stream() {
    let threads = MemoryThreads::new();
    let (handle, open) = opened(&threads.client()).await;
    let writer = handle.writer();
    let entry = entry_id();
    let (_sink, control) = writer.response(entry).expect("a pair");
    handle.keepalive().await.expect("taken");
    let response = text_response(entry, &[(block_id(), "via writer")]);

    control.complete(&response).await.expect("completes");

    assert_eq!(chunk_seqs(&threads), contiguous(4));
    assert_eq!(
        folded(open.message_id, entry, &deltas_sent(&threads)),
        Some(response)
    );
}
