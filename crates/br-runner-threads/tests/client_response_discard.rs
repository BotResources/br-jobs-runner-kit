mod support;

use br_llm_messages::{DeltaOp, ResponseMetadata};
use br_runner_threads::{ClosureKind, Error};
use support::client_streaming::*;
use support::client_x::*;
use support::*;

#[tokio::test]
async fn discarding_a_response_that_sent_a_delta_sends_one_interrupted_delta_with_the_cancelled_reason()
 {
    let mut s = streaming().await;
    s.stream_partial_text(block_id(), "he").await;
    let before = s.sent().len();

    s.control.discard().await.expect("discarded");

    let sent = s.sent();
    assert_eq!(sent.len(), before + 1);
    let last = sent.last().expect("a delta");
    assert!(interrupted_cancelled(last), "{last:?}");
    assert_eq!(last.turn(), s.open.message_id);
    assert_eq!(last.entry(), s.entry);
}

#[tokio::test]
async fn discarding_a_response_that_sent_nothing_sends_nothing() {
    let s = streaming().await;

    s.control.discard().await.expect("accepted");

    assert!(s.threads.chunks().is_empty());
}

#[tokio::test]
async fn discarding_twice_sends_the_interrupted_delta_once() {
    let mut s = streaming().await;
    s.stream_partial_text(block_id(), "he").await;

    s.control.discard().await.expect("discarded");
    let sent = s.threads.chunks().len();
    s.control.discard().await.expect("accepted");

    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn discarding_after_a_completion_sends_nothing() {
    let s = streaming().await;
    s.control
        .complete(&text_response(s.entry, &[(block_id(), "x")]))
        .await
        .expect("completes");
    let sent = s.threads.chunks().len();

    s.control.discard().await.expect("accepted");

    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn a_discard_after_a_kept_invalid_input_still_ends_the_response() {
    let mut s = streaming().await;
    s.stream_partial_text(block_id(), "he").await;
    s.send(DeltaOp::text_appended(block_id(), "refused")).await;
    assert!(matches!(s.control.failure(), Some(Error::InvalidInput(_))));

    s.control.discard().await.expect("discarded");

    assert!(interrupted_cancelled(s.sent().last().expect("a delta")));
}

#[tokio::test]
async fn a_discard_whose_chunk_the_bus_does_not_take_gives_stream_broken() {
    let mut s = streaming().await;
    s.stream_partial_text(block_id(), "he").await;
    s.threads
        .answer_chunks(|_, _, _| Err(Error::Unauthenticated));

    let result = s.control.discard().await;

    assert_eq!(result, Err(Error::StreamBroken));
    assert_eq!(s.control.failure(), Some(Error::StreamBroken));
}

#[tokio::test]
async fn a_discard_with_a_kept_stream_broken_sends_nothing() {
    let mut s = streaming().await;
    s.threads
        .answer_chunks(|_, _, _| Err(Error::Unauthenticated));
    s.send(DeltaOp::response_started(ResponseMetadata::new()))
        .await;
    s.threads.answer_chunks(|_, _, _| Ok(()));
    let sent = s.threads.chunks().len();

    let result = s.control.discard().await;

    assert_eq!(result, Ok(()));
    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn a_discard_after_a_closing_sends_nothing_and_the_control_keeps_stream_closed() {
    let mut s = streaming().await;
    s.stream_partial_text(block_id(), "he").await;
    s.threads
        .client()
        .close_message(
            &closing_of(&s.open, ClosureKind::Cancelled),
            Some(&s.handle),
        )
        .await
        .expect("confirmed");
    let sent = s.threads.chunks().len();

    let result = s.control.discard().await;

    assert_eq!(result, Ok(()));
    assert_eq!(s.threads.chunks().len(), sent);
    assert_eq!(s.control.failure(), Some(Error::StreamClosed));
}

#[tokio::test]
async fn a_discard_after_a_completion_that_failed_midway_is_not_sent_when_the_stream_is_broken() {
    let s = streaming().await;
    s.threads.answer_chunks(|_, seq, _| {
        if seq < 1 {
            Ok(())
        } else {
            Err(Error::Unauthenticated)
        }
    });
    s.control
        .complete(&text_response(s.entry, &[(block_id(), "x")]))
        .await
        .expect_err("broken midway");
    let sent = s.threads.chunks().len();

    let result = s.control.discard().await;

    assert_eq!(result, Ok(()));
    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn a_discarded_response_folds_into_an_interrupted_draft() {
    let mut s = streaming().await;
    s.stream_partial_text(block_id(), "he").await;
    s.control.discard().await.expect("discarded");

    let mut draft = br_llm_messages::ResponseDraft::new(s.open.message_id, s.entry);
    for delta in s.sent() {
        draft.apply(&delta).expect("the sent deltas fold");
    }

    assert!(matches!(
        draft.phase(),
        br_llm_messages::DraftPhase::Interrupted { .. }
    ));
}
