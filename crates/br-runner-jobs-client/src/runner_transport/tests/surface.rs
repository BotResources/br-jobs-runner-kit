use std::time::Duration;

use contract_jobs::reason::Reason;
use contract_jobs::runner_surface::{
    MarkRead, RegisterAgents, SURFACE_WIRE_VERSION, SurfaceReason, SurfaceReply, mark_read_subject,
    register_agents_subject,
};
use contract_jobs::segment::SubjectSegment;
use uuid::Uuid;

use super::live;
use super::live_standin::Responder;
use super::live_world::{WAIT, World};
use crate::backend::Backend;
use crate::error::{Cause, Error, Operation};
use crate::model::AgentDeclaration;

fn segment() -> SubjectSegment {
    SubjectSegment::runner_type("analyst").expect("segment")
}

fn bytes<T: serde::Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).expect("json")
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_accepting_service_when_registering_then_the_wire_request_carries_the_agents() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut service = Responder::start(
        &world.client,
        register_agents_subject(&segment()),
        Some(bytes(&SurfaceReply::accepted())),
    )
    .await;
    let (run_id, agent_id) = (Uuid::now_v7(), Uuid::now_v7());
    live.register_agents(
        run_id,
        vec![AgentDeclaration {
            agent_id,
            has_inbox: true,
            can_restart: false,
        }],
        WAIT,
    )
    .await
    .map_err(|e| e.to_string())
    .expect("accepted");
    let request: RegisterAgents = serde_json::from_slice(&service.request().await).expect("wire");
    assert_eq!(
        (request.version, request.run_id),
        (SURFACE_WIRE_VERSION, run_id)
    );
    assert_eq!(request.instance_key, "pod-1");
    assert_eq!(request.agents.len(), 1);
    assert_eq!(request.agents[0].agent_id, agent_id);
    assert!(request.agents[0].has_inbox);
    assert!(!request.agents[0].declares_restart);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_refusing_service_when_registering_then_refused_with_the_reason_word_and_the_message()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let _service = Responder::start(
        &world.client,
        register_agents_subject(&segment()),
        Some(bytes(&SurfaceReply::refused(
            SurfaceReason::RunEnded,
            "the run is over",
        ))),
    )
    .await;
    let error = live
        .register_agents(Uuid::now_v7(), Vec::new(), WAIT)
        .await
        .expect_err("refused");
    match error {
        Error::Refused(refusal) => {
            assert_eq!(refusal.operation, Operation::Registration);
            assert_eq!(refusal.reason, "run_ended");
            assert_eq!(refusal.message, "the run is over");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_refusal_with_an_unknown_reason_when_registering_then_the_text_of_the_service_is_kept()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let reply = SurfaceReply {
        version: SURFACE_WIRE_VERSION,
        accepted: false,
        reason: Some(Reason::Unknown("brand_new_reason".to_owned())),
        message: None,
    };
    let _service = Responder::start(
        &world.client,
        register_agents_subject(&segment()),
        Some(bytes(&reply)),
    )
    .await;
    let error = live
        .register_agents(Uuid::now_v7(), Vec::new(), WAIT)
        .await
        .expect_err("refused");
    match error {
        Error::Refused(refusal) => assert_eq!(refusal.reason, "brand_new_reason"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_reply_that_is_not_the_wire_shape_when_registering_then_unreadable() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let _service = Responder::start(
        &world.client,
        register_agents_subject(&segment()),
        Some(b"not json".to_vec()),
    )
    .await;
    let error = live
        .register_agents(Uuid::now_v7(), Vec::new(), WAIT)
        .await
        .expect_err("bad");
    assert!(matches!(error, Error::Unreadable(_)), "{error:?}");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_no_service_when_registering_then_unavailable_with_no_listener() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let error = live
        .register_agents(Uuid::now_v7(), Vec::new(), WAIT)
        .await
        .expect_err("nobody");
    match error {
        Error::Unavailable(failure) => assert_eq!(failure.cause, Cause::NoListener),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_silent_service_when_marking_read_then_lost_with_a_fired_wait() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let _silent = Responder::start(&world.client, mark_read_subject(&segment()), None).await;
    let error = live
        .mark_read(
            Uuid::now_v7(),
            vec![Uuid::now_v7()],
            Duration::from_millis(200),
        )
        .await
        .expect_err("no reply");
    match error {
        Error::Lost(failure) => assert_eq!(failure.cause, Cause::WaitFired),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_accepting_and_refusing_services_when_marking_read_then_ok_and_refused_with_the_marker_operation()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut accepting = Responder::start(
        &world.client,
        mark_read_subject(&segment()),
        Some(bytes(&SurfaceReply::accepted())),
    )
    .await;
    let (run_id, item) = (Uuid::now_v7(), Uuid::now_v7());
    live.mark_read(run_id, vec![item], WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("ok");
    let request: MarkRead = serde_json::from_slice(&accepting.request().await).expect("wire");
    assert_eq!((request.run_id, request.item_ids), (run_id, vec![item]));
    assert_eq!(request.instance_key, "pod-1");
    drop(accepting);
    let _refusing = Responder::start(
        &world.client,
        mark_read_subject(&segment()),
        Some(bytes(&SurfaceReply::refused(
            SurfaceReason::ItemNotYours,
            "no",
        ))),
    )
    .await;
    let error = live
        .mark_read(run_id, vec![item], WAIT)
        .await
        .expect_err("refused");
    match error {
        Error::Refused(refusal) => {
            assert_eq!(refusal.operation, Operation::ReadMarker);
            assert_eq!(refusal.reason, "item_not_yours");
        }
        other => panic!("{other:?}"),
    }
}
