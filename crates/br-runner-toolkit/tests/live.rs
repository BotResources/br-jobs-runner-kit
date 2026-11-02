mod live_support;
mod support;

use std::time::Duration;

use async_nats::jetstream::kv::Operation;
use br_runner_jobs::{JobsClient, PresenceStatus, WriteKind};
use br_runner_toolkit::{HostEvent, HostRun, Settings, SettingsBuilder, TestHost};
use contract_jobs::dispatch_mode::{DispatchMode, RunnerTypeDispatchMode, dispatch_mode_key};
use contract_jobs::runner::{
    ClaimReplyV2, ClaimV2, Presence, RunCompleted, RunnerStatus, STATUS_STREAM, TRIGGER_STREAM,
    WIRE_VERSION,
};
use contract_jobs::runner_surface::{StartedRequest, SurfaceReply, started_subject};
use contract_jobs::runner_transport::{RunnerStatusFact, claim_v2_subject, status_subject};
use contract_jobs::segment::SubjectSegment;
use futures::StreamExt;
use live_support::standin::{Responder, publish_trigger, trigger};
use live_support::world::{WAIT, World, eventually};
use support::{Behavior, args, single};
use uuid::Uuid;

const RUNNER_TYPE: &str = "analyst";

fn settings() -> SettingsBuilder {
    Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(1)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(60))
        .cancel_grace(Duration::from_secs(1))
        .claim_wait(Duration::from_secs(5))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1))
}

async fn set_mode(world: &World, mode: DispatchMode) {
    let entry = serde_json::to_vec(&RunnerTypeDispatchMode {
        version: WIRE_VERSION,
        runner_type: RUNNER_TYPE.to_owned(),
        dispatch_mode: mode,
    })
    .expect("json");
    world
        .bucket(br_util_nats_fabric::KV_PUBLISHED_LANGUAGE)
        .await
        .put(dispatch_mode_key(RUNNER_TYPE), entry.into())
        .await
        .expect("the mode is put");
}

async fn host(world: &World) -> TestHost<support::Flow> {
    let client = JobsClient::new(
        support::runner_type(RUNNER_TYPE),
        "pod-1",
        WAIT,
        world.connection(),
    )
    .await
    .map_err(|error| error.to_string())
    .expect("the client connects");
    TestHost::new(
        client,
        settings(),
        args(single(Behavior::Result("x")), false),
    )
    .map_err(|error| error.to_string())
    .expect("a test host")
}

#[derive(Debug, PartialEq)]
enum Seen {
    Put { capacity: u32, version: String },
    Deleted,
}

async fn presence_seen(watch: &mut async_nats::jetstream::kv::Watch) -> Vec<Seen> {
    let mut seen = Vec::new();
    while let Ok(Some(Ok(entry))) = tokio::time::timeout(Duration::from_secs(1), watch.next()).await
    {
        match entry.operation {
            Operation::Put => {
                let presence: Presence = serde_json::from_slice(&entry.value).expect("presence");
                assert_eq!(presence.status, RunnerStatus::Ready);
                seen.push(Seen::Put {
                    capacity: presence.capacity.get(),
                    version: presence.runner_version,
                });
            }
            _ => seen.push(Seen::Deleted),
        }
    }
    seen
}

fn position(events: &[HostEvent], wanted: impl Fn(&HostEvent) -> bool) -> usize {
    events
        .iter()
        .position(wanted)
        .unwrap_or_else(|| panic!("an event is missing in {events:?}"))
}

fn delivered(kind: WriteKind) -> impl Fn(&HostEvent) -> bool {
    move |event| {
        matches!(
            event,
            HostEvent::Queue(br_runner_jobs::QueueReport::Delivered { write }) if write.kind == kind
        )
    }
}

async fn status_facts(world: &World, run_id: Uuid) -> Vec<RunnerStatusFact> {
    let segment = SubjectSegment::runner_type(RUNNER_TYPE).expect("a segment");
    let completed = status_subject(&segment, RunnerStatusFact::Completed);
    world
        .stored(STATUS_STREAM)
        .await
        .into_iter()
        .filter_map(|(subject, payload)| {
            if subject == completed {
                let fact: RunCompleted = serde_json::from_slice(&payload).expect("completed");
                (fact.run_id == run_id).then_some(RunnerStatusFact::Completed)
            } else {
                None
            }
        })
        .collect()
}

async fn started_stand_in(world: &World) -> Responder {
    let segment = SubjectSegment::runner_type(RUNNER_TYPE).expect("a segment");
    let reply = serde_json::to_vec(&SurfaceReply::accepted()).expect("json");
    Responder::start(&world.client, started_subject(&segment), Some(reply)).await
}

async fn assert_started_request(responder: &mut Responder, run_id: Uuid) {
    let request: StartedRequest =
        serde_json::from_slice(&responder.request().await).expect("a started request");
    assert_eq!(request.run_id, run_id);
    assert_eq!(request.instance_key, "pod-1");
}

async fn assert_one_run_in_order(world: &World, run_id: Uuid, run: &HostRun, watch_seen: &[Seen]) {
    assert_eq!(
        watch_seen.first(),
        Some(&Seen::Put {
            capacity: 1,
            version: "1.0.0".to_owned()
        })
    );
    assert_eq!(watch_seen.last(), Some(&Seen::Deleted));
    assert_eq!(
        status_facts(world, run_id).await,
        vec![RunnerStatusFact::Completed]
    );
    let events = &run.events;
    let put = position(events, |event| {
        matches!(
            event,
            HostEvent::PresencePublished { capacity: 1, status: PresenceStatus::Ready, runner_version, .. } if runner_version == "1.0.0"
        )
    });
    let started = position(events, delivered(WriteKind::Started));
    let completed = position(events, delivered(WriteKind::Completed));
    let deleted = position(events, |event| matches!(event, HostEvent::PresenceDeleted));
    assert!(
        put < started && started < completed && completed < deleted,
        "{events:?}"
    );
    assert!(run.drain.runs.is_empty());
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_trigger_on_a_real_bus_when_the_test_host_runs_it_with_the_real_clock_then_the_presence_the_facts_the_acknowledgement_and_the_deletion_come_in_order()
 {
    let world = World::ready().await;
    set_mode(&world, DispatchMode::Push).await;
    let (run_id, job_id) = (Uuid::now_v7(), Uuid::now_v7());
    publish_trigger(&world.js, RUNNER_TYPE, &trigger(run_id, job_id)).await;
    let mut started = started_stand_in(&world).await;
    let mut watch = world
        .presence_bucket()
        .await
        .watch_all()
        .await
        .expect("a watch");
    let host = host(&world).await;
    let run = tokio::time::timeout(Duration::from_secs(60), host.run())
        .await
        .expect("the host returned")
        .map_err(|error| error.to_string())
        .expect("a run");
    let seen = presence_seen(&mut watch).await;
    assert_one_run_in_order(&world, run_id, &run, &seen).await;
    assert_started_request(&mut started, run_id).await;
    let acknowledged = position(&run.events, |event| {
        matches!(event, HostEvent::TriggerAcknowledged)
    });
    let started = position(&run.events, delivered(WriteKind::Started));
    assert!(started < acknowledged, "{:?}", run.events);
    eventually("the trigger is gone from the stream", || async {
        world.messages_in(TRIGGER_STREAM).await == 0
    })
    .await;
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_served_claim_on_a_real_bus_when_the_test_host_runs_it_with_the_real_clock_then_the_claim_the_facts_and_the_presence_deletion_come_in_order()
 {
    let world = World::ready().await;
    set_mode(&world, DispatchMode::Claim).await;
    let (run_id, job_id) = (Uuid::now_v7(), Uuid::now_v7());
    let reply = serde_json::to_vec(&ClaimReplyV2::served(trigger(run_id, job_id))).expect("json");
    let subject = claim_v2_subject(&SubjectSegment::runner_type(RUNNER_TYPE).expect("segment"));
    let mut responder = Responder::start(&world.client, subject, Some(reply)).await;
    let mut started = started_stand_in(&world).await;
    let mut watch = world
        .presence_bucket()
        .await
        .watch_all()
        .await
        .expect("a watch");
    let host = host(&world).await;
    let run = tokio::time::timeout(Duration::from_secs(60), host.run())
        .await
        .expect("the host returned")
        .map_err(|error| error.to_string())
        .expect("a run");
    let seen = presence_seen(&mut watch).await;
    assert_one_run_in_order(&world, run_id, &run, &seen).await;
    assert_started_request(&mut started, run_id).await;
    let request: ClaimV2 = serde_json::from_slice(&responder.request().await).expect("a claim");
    assert_eq!(request.instance_key, "pod-1");
    let claim = position(
        &run.events,
        |event| matches!(event, HostEvent::Claim { claim_id } if *claim_id == request.claim_id),
    );
    let started = position(&run.events, delivered(WriteKind::Started));
    assert!(claim < started, "{:?}", run.events);
    assert!(
        !run.events
            .iter()
            .any(|event| matches!(event, HostEvent::TriggerAcknowledged))
    );
    assert_eq!(world.messages_in(TRIGGER_STREAM).await, 0);
}
