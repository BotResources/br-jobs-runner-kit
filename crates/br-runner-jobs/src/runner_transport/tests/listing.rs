use contract_jobs::runner_surface::{
    AgentState, ListAgents, ListAgentsReply, ListedAgent as WireListed, SURFACE_WIRE_VERSION,
    SurfaceReason, list_agents_subject,
};
use contract_jobs::segment::SubjectSegment;
use uuid::Uuid;

use super::live;
use super::live_standin::Responder;
use super::live_world::{WAIT, World};
use crate::backend::Backend;
use crate::error::{Error, Operation};

fn segment() -> SubjectSegment {
    SubjectSegment::runner_type("analyst").expect("segment")
}

fn bytes<T: serde::Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).expect("json")
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_listing_service_when_listing_then_the_agents_are_mapped_from_the_wire() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    let (first, second, address) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let reply = ListAgentsReply::listed(vec![
        WireListed {
            agent_id: first,
            run_id,
            runner_type: "analyst".to_owned(),
            state: AgentState::Running,
            has_inbox: true,
            mail_address: Some(address),
            restartable_now: None,
        },
        WireListed {
            agent_id: second,
            run_id,
            runner_type: "writer".to_owned(),
            state: AgentState::Finished,
            has_inbox: false,
            mail_address: None,
            restartable_now: Some(true),
        },
    ]);
    let mut service = Responder::start(
        &world.client,
        list_agents_subject(&segment()),
        Some(bytes(&reply)),
    )
    .await;
    let agents = live
        .list_agents(run_id, WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("listed");
    let request: ListAgents = serde_json::from_slice(&service.request().await).expect("wire");
    assert_eq!(
        (request.run_id, request.instance_key.as_str()),
        (run_id, "pod-1")
    );
    assert_eq!(agents.len(), 2);
    assert!(agents[0].running && agents[0].has_inbox);
    assert_eq!(
        (agents[0].agent_id, agents[0].mail_address),
        (first, Some(address))
    );
    assert!(!agents[1].running);
    assert_eq!(
        (agents[1].runner_type.as_str(), agents[1].restartable_now),
        ("writer", Some(true))
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_refused_or_empty_listing_when_listing_then_refused_or_unreadable() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let refusal = ListAgentsReply::refused(SurfaceReason::RunUnknown, "who");
    let service = Responder::start(
        &world.client,
        list_agents_subject(&segment()),
        Some(bytes(&refusal)),
    )
    .await;
    match live
        .list_agents(Uuid::now_v7(), WAIT)
        .await
        .expect_err("refused")
    {
        Error::Refused(refusal) => {
            assert_eq!(refusal.operation, Operation::AgentList);
            assert_eq!(refusal.reason, "run_unknown");
        }
        other => panic!("{other:?}"),
    }
    drop(service);
    let hollow = ListAgentsReply {
        version: SURFACE_WIRE_VERSION,
        accepted: true,
        agents: None,
        reason: None,
        message: None,
    };
    let _service = Responder::start(
        &world.client,
        list_agents_subject(&segment()),
        Some(bytes(&hollow)),
    )
    .await;
    let error = live
        .list_agents(Uuid::now_v7(), WAIT)
        .await
        .expect_err("hollow");
    assert!(matches!(error, Error::Unreadable(_)), "{error:?}");
}
