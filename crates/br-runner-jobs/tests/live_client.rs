#![allow(dead_code)] // the support is shared with the in-crate tests and each file uses part of it

#[path = "support/live_server.rs"]
mod live_server;
#[path = "support/live_standin.rs"]
mod live_standin;
#[path = "support/live_world.rs"]
mod live_world;

use std::time::Duration;

use br_runner_jobs::{
    CancelEvent, Capacity, DispatchMode, Error, JobsClient, LogLevel, ModeEvent, PresenceStatus,
};
use contract_jobs::dispatch_mode::{RunnerTypeDispatchMode, dispatch_mode_key};
use contract_jobs::runner::{
    CANCEL_BUCKET, CancelRun, ClaimReplyV2, LOG_STREAM, LogLine, TRIGGER_STREAM, WIRE_VERSION,
};
use contract_jobs::runner_transport::{claim_v2_subject, run_cancel_key};
use contract_jobs::segment::SubjectSegment;
use futures::StreamExt;
use live_standin::{Responder, publish_trigger, trigger};
use live_world::{PRESENCE_TTL, WAIT, World, eventually};
use uuid::Uuid;

async fn client(world: &World) -> JobsClient {
    JobsClient::new(
        br_runner_jobs::RunnerType::new("analyst").expect("a runner type"),
        "pod-1",
        WAIT,
        world.connection(),
    )
    .await
    .map_err(|error| error.to_string())
    .expect("the client connects")
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_trigger_when_the_client_reads_it_then_the_work_carries_the_run_and_ack_empties_the_queue()
 {
    let world = World::ready().await;
    let client = client(&world).await;
    let mut triggers = client
        .triggers(Duration::from_secs(30))
        .await
        .expect("triggers");
    let (run_id, job_id) = (Uuid::now_v7(), Uuid::now_v7());
    publish_trigger(&world.js, "analyst", &trigger(run_id, job_id)).await;
    let work = tokio::time::timeout(Duration::from_secs(10), triggers.next())
        .await
        .expect("a trigger came")
        .map_err(|unreadable| unreadable.error.to_string())
        .expect("a readable trigger");
    assert_eq!(
        (work.run_id(), work.job_id(), work.attempt()),
        (run_id, job_id, 1)
    );
    work.ack().await.expect("acked");
    assert_eq!(world.messages_in(TRIGGER_STREAM).await, 0);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_trigger_that_is_not_json_when_read_then_it_is_unreadable_and_removed_from_the_queue()
 {
    let world = World::ready().await;
    let client = client(&world).await;
    let mut triggers = client
        .triggers(Duration::from_secs(30))
        .await
        .expect("triggers");
    let subject = contract_jobs::runner_transport::trigger_v2_subject(
        &SubjectSegment::runner_type("analyst").expect("segment"),
    );
    world
        .js
        .publish(subject, b"{".to_vec().into())
        .await
        .expect("sent")
        .await
        .expect("stored");
    let unreadable = tokio::time::timeout(Duration::from_secs(10), triggers.next())
        .await
        .expect("a trigger came")
        .expect_err("unreadable");
    assert!(
        matches!(unreadable.error, Error::Unreadable(_)),
        "{:?}",
        unreadable.error
    );
    eventually("the poison trigger is gone", || async {
        world.messages_in(TRIGGER_STREAM).await == 0
    })
    .await;
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_work_when_it_logs_then_the_log_stream_holds_the_line_of_its_run() {
    let world = World::ready().await;
    let client = client(&world).await;
    let mut triggers = client
        .triggers(Duration::from_secs(30))
        .await
        .expect("triggers");
    let run_id = Uuid::now_v7();
    publish_trigger(&world.js, "analyst", &trigger(run_id, Uuid::now_v7())).await;
    let work = triggers
        .next()
        .await
        .map_err(|u| u.error.to_string())
        .expect("work");
    work.log(LogLevel::Info, "reading", Some(0), None)
        .await
        .expect("logged");
    eventually("the log line is stored", || async {
        world.messages_in(LOG_STREAM).await == 1
    })
    .await;
    let line: LogLine =
        serde_json::from_slice(&world.stored(LOG_STREAM).await[0].1).expect("a LogLine");
    assert_eq!(
        (line.run_id, line.message.as_str(), line.step_index),
        (run_id, "reading", Some(0))
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_presence_when_published_read_and_deleted_then_the_client_sees_the_bucket_rules() {
    let world = World::ready().await;
    let client = client(&world).await;
    client
        .publish_presence(
            "1.0.0",
            PresenceStatus::Ready,
            Capacity::new(2).expect("capacity"),
        )
        .await
        .expect("published");
    assert_eq!(
        client.presence_time_to_live().await.expect("ttl"),
        PRESENCE_TTL
    );
    assert_eq!(
        world
            .presence_bucket()
            .await
            .keys()
            .await
            .expect("keys")
            .count()
            .await,
        1
    );
    client.delete_presence().await.expect("deleted");
    assert_eq!(
        world
            .presence_bucket()
            .await
            .keys()
            .await
            .expect("keys")
            .count()
            .await,
        0
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_listener_when_claiming_then_the_served_run_becomes_a_work_without_a_trigger() {
    let world = World::ready().await;
    let client = client(&world).await;
    let (run_id, job_id) = (Uuid::now_v7(), Uuid::now_v7());
    let reply = serde_json::to_vec(&ClaimReplyV2::served(trigger(run_id, job_id))).expect("json");
    let subject = claim_v2_subject(&SubjectSegment::runner_type("analyst").expect("segment"));
    let _listener = Responder::start(&world.client, subject, Some(reply)).await;
    let work = client
        .claim(Uuid::now_v7(), Duration::from_millis(500))
        .await
        .expect("a reply")
        .expect("a run");
    assert_eq!((work.run_id(), work.job_id()), (run_id, job_id));
    assert!(work.ack().await.is_err());
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_listener_with_nothing_to_serve_when_claiming_then_none() {
    let world = World::ready().await;
    let client = client(&world).await;
    let reply = serde_json::to_vec(&ClaimReplyV2 {
        version: contract_jobs::runner::WIRE_VERSION_V2,
        run: None,
        reason: None,
        retry_after_ms: None,
    })
    .expect("json");
    let subject = claim_v2_subject(&SubjectSegment::runner_type("analyst").expect("segment"));
    let _listener = Responder::start(&world.client, subject, Some(reply)).await;
    assert!(
        client
            .claim(Uuid::now_v7(), Duration::from_millis(500))
            .await
            .expect("a reply")
            .is_none()
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_cancel_orders_when_the_feed_is_read_then_replay_then_made_then_deleted() {
    let world = World::ready().await;
    let client = client(&world).await;
    let stored = Uuid::now_v7();
    let bucket = world.bucket(CANCEL_BUCKET).await;
    let body = |run_id| {
        serde_json::to_vec(&CancelRun {
            version: WIRE_VERSION,
            run_id,
        })
        .expect("json")
    };
    bucket
        .put(run_cancel_key(stored), body(stored).into())
        .await
        .expect("put");
    let mut orders = client.cancel_orders().await.expect("feed");
    assert_eq!(
        orders.next().await,
        Ok(CancelEvent::OrderMade { run_id: stored })
    );
    assert_eq!(orders.next().await, Ok(CancelEvent::ReplayComplete));
    let later = Uuid::now_v7();
    bucket
        .put(run_cancel_key(later), body(later).into())
        .await
        .expect("put");
    assert_eq!(
        orders.next().await,
        Ok(CancelEvent::OrderMade { run_id: later })
    );
    bucket.delete(run_cancel_key(later)).await.expect("delete");
    assert_eq!(
        orders.next().await,
        Ok(CancelEvent::OrderDeleted { run_id: later })
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_dispatch_mode_when_it_changes_then_the_feed_follows() {
    let world = World::ready().await;
    let client = client(&world).await;
    let mut modes = client.dispatch_modes().await.expect("feed");
    assert_eq!(modes.next().await, Ok(ModeEvent::NoEntry));
    let entry = serde_json::to_vec(&RunnerTypeDispatchMode {
        version: WIRE_VERSION,
        runner_type: "analyst".to_owned(),
        dispatch_mode: contract_jobs::dispatch_mode::DispatchMode::Claim,
    })
    .expect("json");
    world
        .bucket(br_util_nats_fabric::KV_PUBLISHED_LANGUAGE)
        .await
        .put(dispatch_mode_key("analyst"), entry.into())
        .await
        .expect("put");
    assert_eq!(modes.next().await, Ok(ModeEvent::Mode(DispatchMode::Claim)));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_missing_stream_when_the_client_is_made_then_infra_missing() {
    let world = World::bare().await;
    let error = JobsClient::new(
        br_runner_jobs::RunnerType::new("analyst").expect("a runner type"),
        "pod-1",
        WAIT,
        world.connection(),
    )
    .await
    .expect_err("refused");
    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
}
