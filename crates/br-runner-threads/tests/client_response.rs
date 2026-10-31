mod support;

use br_agentic_contract::StreamSink;
use br_llm_messages::{DeltaOp, Finalization, ResponseDelta, ResponseEnding, ResponseMetadata};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{ClosureKind, Error};
use support::client_streaming::*;
use support::client_x::*;
use support::*;

#[tokio::test]
async fn opening_a_sink_sends_nothing() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    let operations = threads.operations().len();

    let (_sink, control) = handle.response(entry_id()).expect("a pair");

    assert_eq!(threads.operations().len(), operations);
    assert_eq!(control.failure(), None);
}

#[tokio::test]
async fn the_deltas_a_sink_takes_are_sent_as_delta_chunks_in_order() {
    let mut s = streaming().await;
    let block = block_id();

    s.stream_partial_text(block, "he").await;
    s.send(DeltaOp::text_appended(block, "llo")).await;

    assert_eq!(
        kinds(&s.sent()),
        vec!["started", "block_started", "text_appended", "text_appended"]
    );
    assert_eq!(chunk_seqs(&s.threads), contiguous(4));
    assert_eq!(s.control.failure(), None);
}

#[tokio::test]
async fn a_delta_of_another_turn_is_not_sent_and_the_control_keeps_invalid_input() {
    let mut s = streaming().await;
    let foreign = ResponseDelta::new(
        turn_id(),
        s.entry,
        DeltaOp::response_started(ResponseMetadata::new()),
    );

    s.sink.send(foreign).await;

    assert!(s.threads.chunks().is_empty());
    assert!(matches!(s.control.failure(), Some(Error::InvalidInput(_))));
}

#[tokio::test]
async fn a_delta_of_another_entry_is_not_sent_and_the_control_keeps_invalid_input() {
    let mut s = streaming().await;
    let foreign = ResponseDelta::new(
        s.open.message_id,
        entry_id(),
        DeltaOp::response_started(ResponseMetadata::new()),
    );

    s.sink.send(foreign).await;

    assert!(s.threads.chunks().is_empty());
    assert!(matches!(s.control.failure(), Some(Error::InvalidInput(_))));
}

#[tokio::test]
async fn a_delta_that_is_not_valid_after_the_earlier_ones_is_not_sent() {
    let mut s = streaming().await;
    let block = block_id();

    s.send(DeltaOp::text_appended(block, "no block yet")).await;

    assert!(s.threads.chunks().is_empty());
    assert!(matches!(s.control.failure(), Some(Error::InvalidInput(_))));
}

#[tokio::test]
async fn after_a_refused_delta_the_sink_sends_nothing_more_and_keeps_the_first_failure() {
    let mut s = streaming().await;
    s.send(DeltaOp::text_appended(block_id(), "refused")).await;
    let first = s.control.failure();

    s.send(DeltaOp::response_started(ResponseMetadata::new()))
        .await;
    s.send(DeltaOp::response_started(ResponseMetadata::new()))
        .await;

    assert!(s.threads.chunks().is_empty());
    assert_eq!(s.control.failure(), first);
}

#[tokio::test]
async fn the_delta_that_completes_the_response_is_checked_but_neither_sent_nor_applied() {
    let mut s = streaming().await;
    let block = block_id();
    s.stream_partial_text(block, "done").await;
    s.send(DeltaOp::block_finalized(block, Finalization::accumulated()))
        .await;
    let before = s.threads.chunks().len();

    s.send(DeltaOp::response_completed(ResponseEnding::finished()))
        .await;

    assert_eq!(s.threads.chunks().len(), before);
    assert_eq!(s.control.failure(), None);
    let response = text_response(s.entry, &[(block, "done")]);
    s.control.complete(&response).await.expect("completes");
    assert_eq!(s.folded(), Some(response));
}

#[tokio::test]
async fn a_completing_delta_that_the_draft_refuses_is_kept_as_invalid_input() {
    let mut s = streaming().await;

    s.send(DeltaOp::response_completed(ResponseEnding::finished()))
        .await;

    assert!(s.threads.chunks().is_empty());
    assert!(matches!(s.control.failure(), Some(Error::InvalidInput(_))));
}

#[tokio::test]
async fn a_chunk_the_bus_does_not_take_is_kept_as_stream_broken_and_stops_the_sink() {
    let mut s = streaming().await;
    s.threads
        .answer_chunks(|_, _, _| Err(Error::Unauthenticated));

    s.send(DeltaOp::response_started(ResponseMetadata::new()))
        .await;
    s.threads.answer_chunks(|_, _, _| Ok(()));
    s.send(DeltaOp::response_started(ResponseMetadata::new()))
        .await;

    assert_eq!(s.control.failure(), Some(Error::StreamBroken));
    assert_eq!(s.threads.chunks().len(), 1);
}

#[tokio::test]
async fn a_sink_over_a_closed_handle_keeps_stream_closed() {
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

    assert_eq!(s.control.failure(), Some(Error::StreamClosed));
    assert_eq!(s.threads.chunks().len(), 1);
}

#[tokio::test]
async fn after_a_completion_the_sink_sends_nothing_more() {
    let mut s = streaming().await;
    let response = text_response(s.entry, &[(block_id(), "once")]);
    s.control.complete(&response).await.expect("completes");
    let sent = s.threads.chunks().len();

    s.send(DeltaOp::response_started(ResponseMetadata::new()))
        .await;

    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn after_a_discard_the_sink_sends_nothing_more() {
    let mut s = streaming().await;
    s.stream_partial_text(block_id(), "he").await;
    s.control.discard().await.expect("discarded");
    let sent = s.threads.chunks().len();

    s.send(DeltaOp::text_appended(block_id(), "late")).await;

    assert_eq!(s.threads.chunks().len(), sent);
}
