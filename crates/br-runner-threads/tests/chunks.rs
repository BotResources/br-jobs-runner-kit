mod support;

use std::sync::Arc;
use std::time::Duration;

use br_llm_messages::{
    DeltaOp, EntryId, ModelResponse, ResponseDelta, ResponseMetadata, TurnEntry, TurnId,
};
use br_runner_threads::transport::ChunkMode;
use br_runner_threads::{Annotation, AnnotationBody, ClosureKind, Error};
use contract_threads::{Chunk, ChunkFrame};
use support::*;
use tokio::sync::Notify;

fn frames(world: &World) -> Vec<ChunkFrame> {
    world
        .bus
        .chunks()
        .into_iter()
        .map(|c| serde_json::from_value(c.frame).expect("a frame of the contract"))
        .collect()
}

fn warning(after: EntryId) -> (EntryId, Annotation) {
    (
        after,
        Annotation::new(
            v7(),
            AnnotationBody::Warning {
                code: "PARTIAL".to_owned(),
                text: Some("cut".to_owned()),
            },
        ),
    )
}

#[tokio::test]
async fn opening_is_a_message_command_with_the_streaming_status_and_gives_a_handle() {
    let world = world();
    let request = open_request(&world);
    let handle = world.client.open_stream(&request).await.expect("opened");
    assert_eq!(handle.message_id(), request.message_id.as_uuid());
    let sent = world.bus.commands();
    assert_eq!(
        (sent[0].aggregate, sent[0].verb, sent[0].version),
        ("message", "store", 2)
    );
    assert_eq!(sent[0].payload["message"]["status"], "STREAMING");
    assert_eq!(sent[0].payload["message"]["author_name"], "Analyst");
    assert_eq!(
        sent[0].payload["parent_message_id"],
        serde_json::json!(request.parent_message_id)
    );
}

#[tokio::test]
async fn a_refused_opening_gives_no_handle() {
    let world = world();
    world
        .bus
        .answer_with(|_| br_runner_threads::transport::Reply::Lost);
    assert!(matches!(
        world.client.open_stream(&open_request(&world)).await,
        Err(Error::AnswerLost(_))
    ));
}

#[tokio::test]
async fn chunks_are_numbered_from_zero_without_a_gap_whatever_their_kind() {
    let world = world();
    let handle = open(&world).await;
    let message = handle.message_id();
    handle.delta(&delta_for(message)).await.expect("delta");
    let entry = notice();
    let entry_id = match &entry {
        TurnEntry::RuntimeNotice(n) => n.id(),
        _ => unreachable!(),
    };
    handle.entry(&entry).await.expect("entry");
    let (after, annotation) = warning(entry_id);
    handle
        .annotation(after, &annotation)
        .await
        .expect("annotation");
    handle.keepalive().await.expect("keepalive");
    let frames = frames(&world);
    assert_eq!(
        frames.iter().map(|f| f.seq).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert!(
        frames
            .iter()
            .all(|f| f.key == message && f.accumulator == "turn_content")
    );
    assert!(matches!(frames[0].chunk, Chunk::Delta { .. }));
    assert!(matches!(frames[1].chunk, Chunk::Entry { .. }));
    assert!(matches!(frames[2].chunk, Chunk::Annotation { .. }));
    assert_eq!(frames[3].chunk, Chunk::Keepalive);
}

#[tokio::test]
async fn a_keepalive_carries_no_content_and_takes_a_number() {
    let world = world();
    let handle = open(&world).await;
    handle.keepalive().await.expect("keepalive");
    let wire = &world.bus.chunks()[0].frame;
    assert_eq!(wire["chunk"], serde_json::json!({ "kind": "KEEPALIVE" }));
    assert_eq!(wire["seq"], 0);
}

#[tokio::test]
async fn a_delta_must_name_the_message_as_its_ai_turn() {
    let world = world();
    let handle = open(&world).await;
    let other = delta_for(v7());
    assert!(matches!(
        handle.delta(&other).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(world.bus.chunks().is_empty());
    handle
        .keepalive()
        .await
        .expect("the refusal spent no number");
    assert_eq!(world.bus.chunks()[0].seq, 0);
}

#[tokio::test]
async fn an_entry_chunk_is_neither_a_model_response_nor_a_closure() {
    let world = world();
    let handle = open(&world).await;
    let response = TurnEntry::ModelResponse(
        ModelResponse::new(
            entry_id(),
            vec![br_llm_messages::ContentBlock::Text(
                br_llm_messages::TextBlock::new(
                    br_llm_messages::BlockId::from_uuid(v7()).expect("block"),
                    "hi",
                )
                .expect("text"),
            )],
            br_llm_messages::ResponseEnding::finished(),
            ResponseMetadata::new(),
        )
        .expect("a response"),
    );
    assert!(matches!(
        handle.entry(&response).await,
        Err(Error::InvalidInput(_))
    ));
    let closure = TurnEntry::TurnClosure(br_llm_messages::TurnClosure::new(
        entry_id(),
        br_llm_messages::ClosureReason::completed(),
    ));
    assert!(matches!(
        handle.entry(&closure).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(world.bus.chunks().is_empty());
}

#[tokio::test]
async fn an_annotation_chunk_is_refused_when_its_id_is_not_a_uuidv7() {
    let world = world();
    let handle = open(&world).await;
    let bad = Annotation::new(not_v7(), AnnotationBody::JobReference { job_id: v7() });
    assert!(matches!(
        handle.annotation(entry_id(), &bad).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(world.bus.chunks().is_empty());
}

#[tokio::test]
async fn writers_and_the_handle_share_one_sequence_and_one_seal() {
    let world = world();
    let handle = open(&world).await;
    let writer = handle.writer();
    let second = writer.clone();
    handle.keepalive().await.expect("handle");
    writer.keepalive().await.expect("writer");
    second.entry(&notice()).await.expect("clone");
    handle.keepalive().await.expect("handle again");
    let seqs: Vec<u64> = world.bus.chunks().iter().map(|c| c.seq).collect();
    assert_eq!(seqs, vec![0, 1, 2, 3]);
    let closing = closing(&handle, world.thread, ClosureKind::Completed);
    world
        .client
        .close_message(&closing, Some(&handle))
        .await
        .expect("closed");
    let sent = world.bus.commands().pop().expect("the closing");
    let chunks: Vec<Chunk> = frames(&world).into_iter().map(|f| f.chunk).collect();
    assert_eq!(sent.payload["seal"]["last_seq"], 3);
    assert_eq!(
        sent.payload["seal"]["hash"],
        serde_json::json!(contract_threads::seal_hash(chunks.iter()).expect("hash"))
    );
}

#[tokio::test]
async fn the_sink_factory_gives_a_new_sink_per_request_that_sends_each_delta_as_a_chunk() {
    let world = world();
    let handle = open(&world).await;
    let message = handle.message_id();
    let factory = handle.sink_factory();
    let mut first = factory.sink();
    let mut second = factory.sink();
    first.send(delta_for(message)).await.expect("first");
    second.send(delta_for(message)).await.expect("second");
    let kinds: Vec<_> = frames(&world).into_iter().map(|f| f.chunk).collect();
    assert_eq!(kinds.len(), 2);
    assert!(kinds.iter().all(|c| matches!(c, Chunk::Delta { .. })));
    let foreign = ResponseDelta::new(
        TurnId::from_uuid(v7()).expect("turn"),
        entry_id(),
        DeltaOp::response_started(ResponseMetadata::new()),
    );
    assert!(matches!(
        first.send(foreign).await,
        Err(Error::InvalidInput(_))
    ));
}

#[tokio::test]
async fn a_chunk_that_gets_no_acknowledgement_breaks_the_handle_for_every_user_of_it() {
    let world = world();
    let handle = open(&world).await;
    let writer = handle.writer();
    let mut sink = handle.sink_factory().sink();
    handle.keepalive().await.expect("first");
    world.bus.chunk_mode(ChunkMode::Fail);
    assert_eq!(handle.keepalive().await, Err(Error::StreamBroken));
    world.bus.chunk_mode(ChunkMode::Ack);
    assert_eq!(writer.keepalive().await, Err(Error::StreamBroken));
    assert_eq!(
        sink.send(delta_for(handle.message_id())).await,
        Err(Error::StreamBroken)
    );
    assert_eq!(
        world.bus.chunks().len(),
        2,
        "the spent number is not reused"
    );
}

#[tokio::test]
async fn a_broken_handle_refuses_a_sealed_closing_and_still_sends_a_fail_closing() {
    let world = world();
    let handle = open(&world).await;
    world.bus.chunk_mode(ChunkMode::Fail);
    let _ = handle.keepalive().await;
    let sealed = closing(&handle, world.thread, ClosureKind::Completed);
    assert_eq!(
        world.client.close_message(&sealed, Some(&handle)).await,
        Err(Error::StreamBroken)
    );
    let failed = closing(&handle, world.thread, ClosureKind::Failed);
    world
        .client
        .close_message(&failed, Some(&handle))
        .await
        .expect("fail command");
    assert_eq!(world.bus.commands().last().map(|c| c.verb), Some("fail"));
    assert_eq!(handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
async fn abandoning_a_chunk_after_it_took_a_number_breaks_the_handle_and_before_changes_nothing() {
    let world = world();
    let handle = Arc::new(open(&world).await);
    let release = Arc::new(Notify::new());
    world.bus.chunk_mode(ChunkMode::Hold(Arc::clone(&release)));
    let held = {
        let handle = Arc::clone(&handle);
        tokio::spawn(async move { handle.keepalive().await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    let queued = {
        let handle = Arc::clone(&handle);
        tokio::spawn(async move { handle.keepalive().await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    queued.abort();
    let _ = queued.await;
    held.abort();
    let _ = held.await;
    world.bus.chunk_mode(ChunkMode::Ack);
    assert_eq!(handle.keepalive().await, Err(Error::StreamBroken));
    assert_eq!(world.bus.chunks().len(), 1);
}
