mod support;

use std::time::Duration;

use br_core_integration::IntegrationCommand;
use br_runner_threads::{ClosureKind, Error, MessageStatus, StreamHandle};
use br_util_nats_fabric::command_subject;
use contract_threads::{
    Chunk, ChunkFrame, CompleteMessageV2, FailMessageV2, StoreMessageV2, TURN_CONTENT_ACCUMULATOR,
    ThreadsCommand, chunk_subject, seal_hash,
};
use support::client_closing_x::kind_of;
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
    assert_eq!(
        arrival.subject,
        command_subject(&CompleteMessageV2::coords())
    );
    assert_eq!(arrival.chunks_on_arrival, 2);
    let landed = frames(&bus).await;
    let held: Vec<&Chunk> = landed.iter().map(|(_, frame)| &frame.chunk).collect();
    let complete = arrival.payload::<CompleteMessageV2>();
    let seal = complete.seal;
    let closure = complete.closure.expect("the completion names its closure");
    assert_eq!(
        closure.to_closure_reason().expect("a closure").detail(),
        None
    );
    assert!(matches!(
        closure.to_closure_reason().expect("a closure"),
        br_llm_messages::ClosureReason::Completed { .. }
    ));
    assert_eq!(seal.last_seq, 1);
    assert_eq!(seal.hash, seal_hash(held).expect("a hash"));
    assert_eq!(handle.keepalive().await, Err(Error::StreamClosed));
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn every_closing_kind_that_is_not_a_completion_reaches_threads_as_a_failure_that_names_its_closure()
 {
    let (_server, bus) = infra().await;
    let responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let kinds = [
        (ClosureKind::Cancelled, MessageStatus::Cancelled),
        (ClosureKind::Failed, MessageStatus::Failed),
        (ClosureKind::Interrupted, MessageStatus::Failed),
        (ClosureKind::BudgetExhausted, MessageStatus::Completed),
    ];

    for (kind, status) in kinds {
        let mut closing = closing(&open_stream(v7()), kind);
        closing.detail = Some("the model failed".to_owned());
        let confirmed = client.close_message(&closing, None).await.expect("closed");

        assert_eq!(confirmed.status, Some(status), "{kind:?}");
        let arrival = responder.received_of(closing.command_id).remove(0);
        assert_eq!(arrival.subject, command_subject(&FailMessageV2::coords()));
        assert_eq!(arrival.command_type(), "message.fail");
        let fail = arrival.payload::<FailMessageV2>();
        assert!(fail.seal.is_none(), "{kind:?}");
        let reason = fail
            .closure
            .expect("the failure names its closure")
            .to_closure_reason()
            .expect("the closure reads back");
        assert_eq!(kind_of(&reason), kind);
        assert_eq!(reason.detail(), Some("the model failed"), "{kind:?}");
    }
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

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn an_opening_carries_its_job_acting_user_and_impersonator_to_the_command_stream() {
    let (_server, bus) = infra().await;
    let _responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let mut open = open_stream(v7());
    let (job, acting, impersonator) = (v7(), v7(), v7());
    open.job_id = Some(job);
    open.acting_user_id = Some(acting);
    open.impersonator_id = Some(impersonator);

    client.open_stream(&open).await.expect("opened");

    let commands = stored(&bus, COMMANDS).await;
    assert_eq!(commands.len(), 1);
    assert_eq!(
        commands[0].subject,
        command_subject(&StoreMessageV2::coords())
    );
    let envelope: IntegrationCommand<StoreMessageV2> = commands[0].json();
    assert_eq!(envelope.command_id, open.command_id);
    assert_eq!(envelope.payload.job_id, Some(job));
    assert_eq!(envelope.payload.acting_user_id, Some(acting));
    assert_eq!(envelope.payload.impersonator_id, Some(impersonator));
}
