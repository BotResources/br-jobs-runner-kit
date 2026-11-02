use std::collections::VecDeque;
use std::time::Duration;

use async_nats::jetstream::consumer::pull::{Config, MessagesError, MessagesErrorKind};
use br_core_kernel::Actor;
use br_util_nats_fabric::{EventMetadata, INTEGRATION_EVT, IntegrationEvent};
use chrono::Utc;
use contract_jobs::event_v2::{ItemKind as WireKind, RunItemArrivedV2};
use uuid::Uuid;

use super::live_world::{World, account};
use crate::backend::{BoxFuture, RawFeed};
use crate::error::Error;
use crate::runner_transport::arrivals::{Delivered, Doorbell, Pull, Pulled};

struct Script(VecDeque<Pulled>);

impl Pull for Script {
    fn pull(&mut self) -> BoxFuture<'_, Pulled> {
        Box::pin(async move { self.0.pop_front().unwrap_or(None) })
    }
}

fn doorbell_of(run_id: Uuid, item_id: Uuid) -> Delivered {
    let metadata = EventMetadata::new(Actor::Service(account()), Uuid::now_v7());
    let event = IntegrationEvent::new(
        Uuid::now_v7(),
        "stand-in",
        1,
        Utc::now(),
        metadata,
        RunItemArrivedV2 {
            run_id,
            address: None,
            item_id,
            item_kind: WireKind::Mail,
        },
    );
    Delivered {
        payload: serde_json::to_vec(&event).expect("json"),
        subject: "integration.evt.jobs.run.item_arrived.v2".to_owned(),
    }
}

fn missed() -> Pulled {
    Some(Err(MessagesError::new(MessagesErrorKind::MissingHeartbeat)))
}

async fn live_consumer(world: &World) -> String {
    let stream = world.js.get_stream(INTEGRATION_EVT).await.expect("stream");
    let consumer = stream
        .create_consumer(Config::default())
        .await
        .expect("a consumer");
    consumer.cached_info().name.clone()
}

fn feed(world: &World, run_id: Uuid, consumer: &str, script: Vec<Pulled>) -> Doorbell {
    Doorbell::new(
        run_id,
        Box::new(Script(script.into())),
        world.js.clone(),
        consumer.to_owned(),
    )
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_missed_heartbeat_when_the_consumer_is_alive_then_the_feed_goes_on() {
    let world = World::ready().await;
    let (run_id, item_id) = (Uuid::now_v7(), Uuid::now_v7());
    let name = live_consumer(&world).await;
    let mut doorbell = feed(
        &world,
        run_id,
        &name,
        vec![missed(), missed(), Some(Ok(doorbell_of(run_id, item_id)))],
    );
    let found = tokio::time::timeout(Duration::from_secs(10), doorbell.next())
        .await
        .expect("the feed gave an item in time");
    assert_eq!(found.map(|item| item.item_id), Ok(item_id));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_missed_heartbeat_when_the_consumer_is_gone_then_the_feed_ends_with_lost_and_stays_ended()
 {
    let world = World::ready().await;
    let run_id = Uuid::now_v7();
    let mut doorbell = feed(
        &world,
        run_id,
        "no-such-consumer",
        vec![missed(), Some(Ok(doorbell_of(run_id, Uuid::now_v7())))],
    );
    let error = doorbell.next().await.expect_err("ended");
    assert!(matches!(error, Error::Lost(_)), "{error:?}");
    assert_eq!(doorbell.next().await.expect_err("still ended"), error);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_missed_heartbeat_when_the_stream_is_gone_then_the_feed_ends_with_infra_missing() {
    let world = World::ready().await;
    let name = live_consumer(&world).await;
    world.delete_stream(INTEGRATION_EVT).await;
    let mut doorbell = feed(&world, Uuid::now_v7(), &name, vec![missed()]);
    let error = doorbell.next().await.expect_err("ended");
    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_pull_that_closed_then_the_feed_ends_with_lost_whether_the_consumer_lives_or_not() {
    let world = World::ready().await;
    let alive = live_consumer(&world).await;
    for name in [alive.as_str(), "no-such-consumer"] {
        let mut doorbell = feed(&world, Uuid::now_v7(), name, Vec::new());
        let error = doorbell.next().await.expect_err("ended");
        assert!(matches!(error, Error::Lost(_)), "{name}: {error:?}");
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_pull_that_failed_when_the_consumer_lives_then_the_feed_ends_with_lost() {
    let world = World::ready().await;
    let name = live_consumer(&world).await;
    let failed = Some(Err(MessagesError::new(MessagesErrorKind::Pull)));
    let mut doorbell = feed(&world, Uuid::now_v7(), &name, vec![failed]);
    let error = doorbell.next().await.expect_err("ended");
    assert!(matches!(error, Error::Lost(_)), "{error:?}");
}
