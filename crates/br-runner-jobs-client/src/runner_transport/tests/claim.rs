use std::time::Duration;

use contract_jobs::runner::{ClaimV2, WIRE_VERSION_V2};
use contract_jobs::runner_transport::claim_v2_subject;
use contract_jobs::segment::SubjectSegment;
use uuid::Uuid;

use super::live;
use super::live_standin::Responder;
use super::live_world::World;
use crate::backend::Backend;
use crate::error::{Cause, Error};

fn subject(runner_type: &str) -> String {
    claim_v2_subject(&SubjectSegment::runner_type(runner_type).expect("segment"))
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_listener_when_claiming_then_the_request_is_the_wire_claim_and_the_reply_bytes_pass_through()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut listener = Responder::start(
        &world.client,
        subject("analyst"),
        Some(b"opaque reply".to_vec()),
    )
    .await;
    let claim_id = Uuid::now_v7();
    let reply = live
        .claim(claim_id, Duration::from_millis(1500))
        .await
        .map_err(|e| e.to_string())
        .expect("a reply");
    assert_eq!(reply, b"opaque reply");
    let request: ClaimV2 = serde_json::from_slice(&listener.request().await).expect("a ClaimV2");
    assert_eq!(request.version, WIRE_VERSION_V2);
    assert_eq!(request.instance_key, "pod-1");
    assert_eq!(request.claim_id, claim_id);
    assert_eq!(request.wait_ms, 1500);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_no_listener_when_claiming_then_unavailable_with_no_listener() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let error = live
        .claim(Uuid::now_v7(), Duration::from_millis(200))
        .await
        .expect_err("refused");
    match error {
        Error::Unavailable(failure) => assert_eq!(failure.cause, Cause::NoListener),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_listener_that_never_replies_when_claiming_then_lost_with_a_fired_wait() {
    let world = World::ready().await;
    let live = super::live_with_wait(&world, "analyst", "pod-1", Duration::from_millis(300)).await;
    let _silent = Responder::start(&world.client, subject("analyst"), None).await;
    let error = live
        .claim(Uuid::now_v7(), Duration::from_millis(100))
        .await
        .expect_err("no reply");
    match error {
        Error::Lost(failure) => assert_eq!(failure.cause, Cause::WaitFired),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_two_runner_types_when_claiming_then_the_request_goes_to_its_own_subject_only() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut other = Responder::start(&world.client, subject("writer"), Some(b"x".to_vec())).await;
    let mut own = Responder::start(&world.client, subject("analyst"), Some(b"mine".to_vec())).await;
    let reply = live
        .claim(Uuid::now_v7(), Duration::from_millis(500))
        .await
        .map_err(|e| e.to_string())
        .expect("a reply");
    assert_eq!(reply, b"mine");
    own.request().await;
    assert!(other.requests.try_recv().is_err());
}
