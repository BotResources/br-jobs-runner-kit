mod support;

use std::time::Duration;

use br_runner_threads::{ClosureKind, Error, MessageStatus, StreamHandle};
use br_util_nats_fabric::command_subject;
use contract_threads::{
    CancelMessage, Chunk, ChunkFrame, CompleteMessage, TURN_CONTENT_ACCUMULATOR, ThreadsCommand,
    chunk_subject, seal_hash,
};
use support::nats_fixtures::*;
use support::nats_responder::Responder;
use support::nats_server::Server;
use support::nats_streams::*;
use support::{notice, v7};

async fn infra() -> (Server, async_nats::Client) {
    let server = Server::start();
    let bus = server.client().await;
    create_all_streams(&bus).await;
    (server, bus)
}

async fn frames(bus: &async_nats::Client) -> Vec<(Stored, ChunkFrame)> {
    stored(bus, CHUNKS)
        .await
        .into_iter()
        .map(|message| {
            let frame = message.json();
            (message, frame)
        })
        .collect()
}

async fn opened(bus: &async_nats::Client) -> (br_runner_threads::OpenStream, StreamHandle) {
    let client = built(bus).await;
    let open = open_stream(v7());
    let handle = client.open_stream(&open).await.expect("opened");
    (open, handle)
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn chunks_land_in_the_chunk_stream_with_their_subject_dedup_id_and_contiguous_numbers() {
    let (_server, bus) = infra().await;
    let _responder = Responder::start(&bus).await;
    let (open, handle) = opened(&bus).await;
    let entry = notice("hello");

    handle.keepalive().await.expect("taken");
    handle.entry(&entry).await.expect("taken");
    handle.keepalive().await.expect("taken");

    let message_id = open.message_id.as_uuid();
    let landed = frames(&bus).await;
    assert_eq!(landed.len(), 3);
    for (position, (message, frame)) in landed.iter().enumerate() {
        let dedup = message.headers.get(async_nats::header::NATS_MESSAGE_ID);
        assert_eq!(message.subject, chunk_subject(message_id));
        assert_eq!(
            dedup.map(|value| value.as_str()),
            Some(&*format!("{message_id}:{position}"))
        );
        assert_eq!(frame.seq, position as u64);
        assert_eq!(frame.key, message_id);
        assert_eq!(frame.accumulator, TURN_CONTENT_ACCUMULATOR);
    }
    assert_eq!(landed[0].1.chunk, Chunk::Keepalive);
    assert_eq!(landed[1].1.chunk, Chunk::entry(&entry).expect("a chunk"));
    assert_eq!(landed[2].1.chunk, Chunk::Keepalive);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn chunks_of_concurrent_writers_reach_the_stream_in_order_with_no_gap() {
    let (_server, bus) = infra().await;
    let _responder = Responder::start(&bus).await;
    let (_open, handle) = opened(&bus).await;
    let writers: Vec<_> = (0..20).map(|_| handle.writer()).collect();

    let results = futures::future::join_all(writers.iter().map(|w| w.keepalive())).await;

    assert!(results.iter().all(Result::is_ok));
    let numbers: Vec<u64> = frames(&bus).await.iter().map(|(_, f)| f.seq).collect();
    assert_eq!(numbers, (0..20).collect::<Vec<u64>>());
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_completed_closing_carries_the_seal_of_the_chunks_the_stream_holds() {
    let (_server, bus) = infra().await;
    let responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let open = open_stream(v7());
    let handle = client.open_stream(&open).await.expect("opened");
    handle.keepalive().await.expect("taken");
    handle.entry(&notice("hello")).await.expect("taken");
    let closing = closing(&open, ClosureKind::Completed);

    let confirmed = client
        .close_message(&closing, Some(&handle))
        .await
        .expect("closed");

    assert_eq!(confirmed.status, Some(MessageStatus::Completed));
    let arrival = responder.received_of(closing.command_id).remove(0);
    assert_eq!(arrival.subject, command_subject(&CompleteMessage::coords()));
    assert_eq!(arrival.chunks_on_arrival, 2);
    let landed = frames(&bus).await;
    let held: Vec<&Chunk> = landed.iter().map(|(_, frame)| &frame.chunk).collect();
    let seal = arrival.payload::<CompleteMessage>().seal;
    assert_eq!(seal.last_seq, 1);
    assert_eq!(seal.hash, seal_hash(held).expect("a hash"));
    assert_eq!(handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn every_closing_kind_without_a_handle_reaches_threads_as_its_command_with_no_seal() {
    let (_server, bus) = infra().await;
    let responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let kinds = [
        (
            ClosureKind::Cancelled,
            "message.cancel",
            MessageStatus::Cancelled,
        ),
        (ClosureKind::Failed, "message.fail", MessageStatus::Failed),
        (
            ClosureKind::Interrupted,
            "message.fail",
            MessageStatus::Failed,
        ),
        (
            ClosureKind::BudgetExhausted,
            "message.fail",
            MessageStatus::Failed,
        ),
    ];

    for (kind, command_type, status) in kinds {
        let mut closing = closing(&open_stream(v7()), kind);
        closing.detail = (kind == ClosureKind::Failed).then(|| "the model failed".to_owned());
        let confirmed = client.close_message(&closing, None).await.expect("closed");

        assert_eq!(confirmed.status, Some(status), "{kind:?}");
        let arrival = responder.received_of(closing.command_id).remove(0);
        assert_eq!(arrival.command_type(), command_type);
        assert!(arrival.envelope.payload.get("seal").is_none(), "{kind:?}");
    }
    let cancel = command_subject(&CancelMessage::coords());
    assert_eq!(responder.received()[0].subject, cancel);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_chunk_the_bus_does_not_take_breaks_the_handle_and_refuses_the_completed_closing() {
    let (_server, bus) = infra().await;
    let _responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let open = open_stream(v7());
    let handle = client.open_stream(&open).await.expect("opened");
    handle.keepalive().await.expect("taken");
    delete_stream(&bus, CHUNKS).await;

    let lost = handle.keepalive().await;
    let later = handle.entry(&notice("later")).await;
    let sealed = client
        .close_message(&closing(&open, ClosureKind::Completed), Some(&handle))
        .await;

    assert_eq!(lost, Err(Error::StreamBroken));
    assert_eq!(later, Err(Error::StreamBroken));
    assert_eq!(sealed.map(drop), Err(Error::StreamBroken));
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_chunk_after_the_server_went_away_is_stream_broken_within_the_wait() {
    let (server, bus) = infra().await;
    let _responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let mut open = open_stream(v7());
    open.wait = Duration::from_secs(1);
    let handle = client.open_stream(&open).await.expect("opened");
    drop(server);

    let result = tokio::time::timeout(Duration::from_secs(5), handle.keepalive())
        .await
        .expect("the chunk ended within its wait");

    assert_eq!(result, Err(Error::StreamBroken));
}
