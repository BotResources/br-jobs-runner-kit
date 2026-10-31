mod support;

use br_llm_messages::{DeltaOp, Finalization};
use br_runner_threads::Error;
use support::client_streaming::*;
use support::client_x::*;
use support::*;

#[tokio::test]
async fn completing_from_an_empty_draft_sends_started_each_block_in_order_then_completed() {
    let s = streaming().await;
    let (first, second) = (block_id(), block_id());
    let response = text_response(s.entry, &[(first, "one"), (second, "two")]);

    s.control.complete(&response).await.expect("completes");

    assert_eq!(
        kinds(&s.sent()),
        vec!["started", "inserted", "inserted", "completed"]
    );
    assert_eq!(chunk_seqs(&s.threads), contiguous(4));
    assert_eq!(s.folded(), Some(response));
}

#[tokio::test]
async fn completing_from_an_empty_draft_invents_no_fragment_of_text() {
    let s = streaming().await;
    let response = text_response(s.entry, &[(block_id(), "whole")]);

    s.control.complete(&response).await.expect("completes");

    let appended = s
        .sent()
        .iter()
        .filter(|delta| matches!(delta.op(), DeltaOp::TextAppended { .. }))
        .count();
    assert_eq!(appended, 0);
}

#[tokio::test]
async fn completing_a_response_with_a_tool_call_folds_back_to_the_response() {
    let s = streaming().await;
    let response = response_call(s.entry, block_id(), "search");

    s.control.complete(&response).await.expect("completes");

    assert_eq!(s.folded(), Some(response));
}

#[tokio::test]
async fn completing_after_a_stream_finalizes_each_open_block_with_the_whole_block() {
    let mut s = streaming().await;
    let block = block_id();
    s.stream_partial_text(block, "he").await;
    let response = text_response(s.entry, &[(block, "hello")]);

    s.control.complete(&response).await.expect("completes");

    let sent = s.sent();
    assert_eq!(
        kinds(&sent),
        vec![
            "started",
            "block_started",
            "text_appended",
            "finalized",
            "completed"
        ]
    );
    assert_eq!(s.folded(), Some(response));
}

#[tokio::test]
async fn completing_after_a_stream_inserts_the_blocks_the_draft_does_not_hold() {
    let mut s = streaming().await;
    let (streamed, extra) = (block_id(), block_id());
    s.stream_partial_text(streamed, "he").await;
    let response = text_response(s.entry, &[(streamed, "hello"), (extra, "more")]);

    s.control.complete(&response).await.expect("completes");

    assert_eq!(
        kinds(&s.sent()),
        vec![
            "started",
            "block_started",
            "text_appended",
            "finalized",
            "inserted",
            "completed"
        ]
    );
    assert_eq!(s.folded(), Some(response));
}

#[tokio::test]
async fn completing_after_a_stream_that_finalized_its_block_does_not_finalize_it_again() {
    let mut s = streaming().await;
    let block = block_id();
    s.stream_partial_text(block, "same").await;
    s.send(DeltaOp::block_finalized(block, Finalization::accumulated()))
        .await;
    let response = text_response(s.entry, &[(block, "same")]);

    s.control.complete(&response).await.expect("completes");

    assert_eq!(
        kinds(&s.sent()),
        vec![
            "started",
            "block_started",
            "text_appended",
            "finalized",
            "completed"
        ]
    );
    assert_eq!(s.folded(), Some(response));
}

#[tokio::test]
async fn completing_after_a_stream_with_several_open_blocks_finalizes_all_of_them() {
    let mut s = streaming().await;
    let (first, second) = (block_id(), block_id());
    s.stream_partial_text(first, "a").await;
    s.send(DeltaOp::block_started(
        second,
        br_llm_messages::BlockStart::text(),
    ))
    .await;
    s.send(DeltaOp::text_appended(second, "b")).await;
    let response = text_response(s.entry, &[(first, "aa"), (second, "bb")]);

    s.control.complete(&response).await.expect("completes");

    assert_eq!(s.folded(), Some(response));
}

#[tokio::test]
async fn the_deltas_sent_then_the_whole_response_deltas_fold_to_the_response() {
    let mut s = streaming().await;
    let block = block_id();
    s.stream_partial_text(block, "par").await;
    let response = text_response(s.entry, &[(block, "partial and more")]);

    s.control.complete(&response).await.expect("completes");

    let Some(folded) = s.folded() else {
        panic!("the deltas do not fold to a response");
    };
    assert_eq!(folded, response);
}

#[tokio::test]
async fn a_whole_response_that_does_not_start_with_the_blocks_of_the_draft_is_refused_and_sends_nothing()
 {
    let mut s = streaming().await;
    s.stream_partial_text(block_id(), "he").await;
    let sent = s.threads.chunks().len();
    let response = text_response(s.entry, &[(block_id(), "other")]);

    let result = s.control.complete(&response).await;

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn a_whole_response_with_fewer_blocks_than_the_draft_is_refused_and_sends_nothing() {
    let mut s = streaming().await;
    let (first, second) = (block_id(), block_id());
    s.stream_partial_text(first, "a").await;
    s.send(DeltaOp::block_started(
        second,
        br_llm_messages::BlockStart::text(),
    ))
    .await;
    let sent = s.threads.chunks().len();
    let response = text_response(s.entry, &[(first, "a")]);

    let result = s.control.complete(&response).await;

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn a_whole_response_that_differs_from_a_finalized_block_is_refused_and_sends_nothing() {
    let mut s = streaming().await;
    let block = block_id();
    s.stream_partial_text(block, "fixed").await;
    s.send(DeltaOp::block_finalized(block, Finalization::accumulated()))
        .await;
    let sent = s.threads.chunks().len();
    let response = text_response(s.entry, &[(block, "changed")]);

    let result = s.control.complete(&response).await;

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn a_refused_completion_leaves_the_response_open_for_a_right_one() {
    let mut s = streaming().await;
    let block = block_id();
    s.stream_partial_text(block, "he").await;
    let wrong = text_response(s.entry, &[(block_id(), "x")]);
    s.control.complete(&wrong).await.expect_err("refused");
    let right = text_response(s.entry, &[(block, "hello")]);

    s.control.complete(&right).await.expect("completes");

    assert_eq!(s.folded(), Some(right));
}

#[tokio::test]
async fn after_a_completion_complete_is_refused_and_sends_nothing() {
    let s = streaming().await;
    let response = text_response(s.entry, &[(block_id(), "once")]);
    s.control.complete(&response).await.expect("completes");
    let sent = s.threads.chunks().len();

    let again = s.control.complete(&response).await;

    assert!(matches!(again, Err(Error::InvalidInput(_))), "{again:?}");
    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn after_a_discard_complete_is_refused_and_sends_nothing() {
    let mut s = streaming().await;
    s.stream_partial_text(block_id(), "he").await;
    s.control.discard().await.expect("discarded");
    let sent = s.threads.chunks().len();

    let result = s
        .control
        .complete(&text_response(s.entry, &[(block_id(), "x")]))
        .await;

    assert!(matches!(result, Err(Error::InvalidInput(_))), "{result:?}");
    assert_eq!(s.threads.chunks().len(), sent);
}

#[tokio::test]
async fn completing_from_an_empty_draft_starts_the_response_with_its_metadata() {
    let s = streaming().await;
    let response = text_response_by_model(s.entry, block_id(), "hi", "model-a");

    s.control.complete(&response).await.expect("completes");

    let sent = s.sent();
    assert!(matches!(
        sent[0].op(),
        DeltaOp::ResponseStarted { metadata, .. } if *metadata == response.metadata()
    ));
    assert_eq!(s.folded(), Some(response));
}

#[tokio::test]
async fn completing_after_a_stream_sends_the_metadata_the_draft_does_not_hold() {
    let mut s = streaming().await;
    let block = block_id();
    s.stream_partial_text(block, "he").await;
    let response = text_response_by_model(s.entry, block, "hello", "model-b");

    s.control.complete(&response).await.expect("completes");

    let kinds = kinds(&s.sent());
    assert!(kinds.contains(&"metadata"), "{kinds:?}");
    assert_eq!(s.folded(), Some(response));
}
