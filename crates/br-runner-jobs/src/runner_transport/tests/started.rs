use std::time::Duration;

use contract_jobs::runner_surface::{
    SURFACE_WIRE_VERSION, StartedRequest, SurfaceReason, SurfaceReply, started_subject,
};
use contract_jobs::runner_transport::{RunnerStatusFact, status_subject};
use contract_jobs::segment::SubjectSegment;
use uuid::Uuid;

use super::live;
use super::live_standin::Responder;
use super::live_world::{WAIT, World};
use crate::backend::Backend;
use crate::error::{Cause, Error, Operation};
use crate::reason::RefusalReason;

async fn on_status_stream(world: &World) -> usize {
    let wanted = status_subject(&started_segment(), RunnerStatusFact::Started);
    world
        .stored(contract_jobs::runner::STATUS_STREAM)
        .await
        .into_iter()
        .filter(|(subject, _)| *subject == wanted)
        .count()
}

fn started_segment() -> SubjectSegment {
    SubjectSegment::runner_type("analyst").expect("segment")
}

fn reply_bytes(reply: &SurfaceReply) -> Vec<u8> {
    serde_json::to_vec(reply).expect("json")
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_accepting_service_when_the_start_is_sent_then_it_is_one_request_with_the_start_id_and_nothing_is_published_on_the_status_stream()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut service = Responder::start(
        &world.client,
        started_subject(&started_segment()),
        Some(reply_bytes(&SurfaceReply::accepted())),
    )
    .await;
    let (run_id, start_id) = (Uuid::now_v7(), Uuid::now_v7());
    live.start(run_id, start_id, WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("accepted");
    let request: StartedRequest = serde_json::from_slice(&service.request().await).expect("wire");
    assert_eq!(request.version, SURFACE_WIRE_VERSION);
    assert_eq!(request.run_id, run_id);
    assert_eq!(request.instance_key, "pod-1");
    assert_eq!(request.start_id, start_id);
    assert_eq!(on_status_stream(&world).await, 0);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_refusing_service_when_the_start_is_sent_then_refused_names_the_typed_reason_and_the_text_of_jobs()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    for (reason, expected) in [
        (
            SurfaceReason::InstanceUnknown,
            RefusalReason::InstanceUnknown,
        ),
        (SurfaceReason::RunHeld, RefusalReason::RunHeld),
        (
            SurfaceReason::RunReservedElsewhere,
            RefusalReason::RunReservedElsewhere,
        ),
        (SurfaceReason::RunNotPending, RefusalReason::RunNotPending),
        (SurfaceReason::RunUnknown, RefusalReason::RunUnknown),
        (SurfaceReason::Malformed, RefusalReason::Malformed),
    ] {
        let _service = Responder::start(
            &world.client,
            started_subject(&started_segment()),
            Some(reply_bytes(&SurfaceReply::refused(
                reason,
                "in plain words",
            ))),
        )
        .await;
        match live.start(Uuid::now_v7(), Uuid::now_v7(), WAIT).await {
            Err(Error::Refused(refusal)) => {
                assert_eq!(refusal.operation, Operation::StartedFact);
                assert_eq!(refusal.reason, expected);
                assert_eq!(refusal.message, "in plain words");
            }
            other => panic!("{other:?}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_no_service_when_the_start_is_sent_then_unavailable_because_nobody_listens() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    match live.start(Uuid::now_v7(), Uuid::now_v7(), WAIT).await {
        Err(Error::Unavailable(failure)) => assert_eq!(failure.cause, Cause::NoListener),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_service_that_does_not_answer_when_the_start_is_sent_then_lost_because_the_wait_fired()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let _service = Responder::start(&world.client, started_subject(&started_segment()), None).await;
    match live
        .start(Uuid::now_v7(), Uuid::now_v7(), Duration::from_millis(300))
        .await
    {
        Err(Error::Lost(failure)) => assert_eq!(failure.cause, Cause::WaitFired),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_reply_that_cannot_be_read_when_the_start_is_sent_then_unreadable() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let _service = Responder::start(
        &world.client,
        started_subject(&started_segment()),
        Some(b"not json".to_vec()),
    )
    .await;
    assert!(matches!(
        live.start(Uuid::now_v7(), Uuid::now_v7(), WAIT).await,
        Err(Error::Unreadable(_))
    ));
}
