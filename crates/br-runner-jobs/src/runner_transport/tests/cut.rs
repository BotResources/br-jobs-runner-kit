use std::time::Duration;

use contract_jobs::event_v2::{ItemKind as WireKind, RunItemArrivedV2};
use contract_jobs::runner::{CANCEL_BUCKET, TRIGGER_STREAM};
use contract_jobs::runner_transport::run_cancel_key;
use uuid::Uuid;

use super::Live;
use super::arrivals::arrive;
use super::cut_proxy::Proxy;
use super::live;
use super::live_standin::{publish_trigger, trigger, try_publish_event};
use super::live_world::{WAIT, World, connection_on, eventually};
use super::next;
use crate::backend::Backend;
use crate::error::Error;
use crate::model::CancelEvent;
use br_util_nats_fabric::INTEGRATION_EVT;

async fn cut(world: &mut World) {
    world.server.restart();
    eventually("the bus answers again", || async {
        tokio::time::timeout(Duration::from_secs(1), world.js.get_stream(TRIGGER_STREAM))
            .await
            .is_ok_and(|found| found.is_ok())
    })
    .await;
}

fn survives_or_lost<T>(outcome: &Result<T, Error>) {
    if let Err(error) = outcome {
        assert!(matches!(error, Error::Lost(_)), "{error:?}");
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_trigger_source_when_the_bus_is_cut_then_it_goes_on_without_loss_or_ends_with_a_typed_error()
 {
    let mut world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut source = live
        .open_triggers(Duration::from_secs(30))
        .await
        .map_err(|e| e.to_string())
        .expect("source");
    cut(&mut world).await;
    let (run_id, job_id) = (Uuid::now_v7(), Uuid::now_v7());
    publish_trigger(&world.js, "analyst", &trigger(run_id, job_id)).await;
    let outcome = tokio::time::timeout(Duration::from_secs(20), source.next())
        .await
        .expect("the source answers after a cut");
    survives_or_lost(&outcome);
    if let Ok(incoming) = outcome {
        let wire: contract_jobs::runner::TriggerV2 =
            serde_json::from_slice(&incoming.raw).expect("wire");
        assert_eq!(wire.run_id, run_id);
        incoming
            .delivery
            .ack()
            .await
            .map_err(|e| e.to_string())
            .expect("acked");
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_cancel_feed_when_the_bus_is_cut_then_it_goes_on_without_loss_or_ends_with_a_typed_error()
 {
    let mut world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .cancel_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::ReplayComplete));
    cut(&mut world).await;
    let run_id = Uuid::now_v7();
    let body = serde_json::to_vec(&contract_jobs::runner::CancelRun {
        version: contract_jobs::runner::WIRE_VERSION,
        run_id,
    })
    .expect("json");
    world
        .bucket(CANCEL_BUCKET)
        .await
        .put(run_cancel_key(run_id), body.into())
        .await
        .expect("put");
    let outcome = tokio::time::timeout(Duration::from_secs(30), feed.next())
        .await
        .expect("answers after a cut");
    survives_or_lost(&outcome);
    if let Ok(event) = outcome {
        assert_eq!(event, CancelEvent::OrderMade { run_id });
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_arrival_feed_that_has_read_an_event_when_the_bus_is_cut_for_a_short_time_then_the_next_event_comes_and_only_a_repeat_may_come_before_it()
 {
    let mut world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    let mut feed = live
        .arrival_feed(run_id)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let (before, after) = (Uuid::now_v7(), Uuid::now_v7());
    arrive(&world, run_id, before, WireKind::Mail, None).await;
    assert_eq!(next(&mut feed).await.map(|item| item.item_id), Ok(before));
    cut(&mut world).await;
    arrive(&world, run_id, after, WireKind::Mail, None).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let item = tokio::time::timeout_at(deadline, feed.next())
            .await
            .expect("the feed answers after a cut")
            .map(|item| item.item_id);
        if item == Ok(after) {
            break;
        }
        assert_eq!(
            item,
            Ok(before),
            "only a repeat of the first event may come"
        );
    }
}

struct Cycle {
    stored: Vec<Uuid>,
    received: Vec<Uuid>,
    ended: Option<Error>,
}

async fn cycle(world: &mut World, live: &crate::runner_transport::Live) -> Cycle {
    let run_id = Uuid::now_v7();
    let mut feed = live
        .arrival_feed(run_id)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let (events, mut arrived) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let outcome = feed.next().await;
            let ended = outcome.is_err();
            if events.send(outcome.map(|item| item.item_id)).is_err() || ended {
                break;
            }
        }
    });
    let js = world.js.clone();
    let publisher = tokio::spawn(async move {
        let coords = contract_jobs::evt_run_item_arrived_v2_coords().expect("coords");
        let mut stored = Vec::new();
        for _ in 0..40 {
            let item_id = Uuid::now_v7();
            let payload = RunItemArrivedV2 {
                run_id,
                address: None,
                item_id,
                item_kind: WireKind::Mail,
            };
            if try_publish_event(&js, &coords, Uuid::now_v7(), payload)
                .await
                .is_ok()
            {
                stored.push(item_id);
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        stored
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    cut(world).await;
    let stored = publisher.await.expect("the publisher ends");
    let mut received = Vec::new();
    let mut ended = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    while ended.is_none() && !stored.iter().all(|id| received.contains(id)) {
        match tokio::time::timeout_at(deadline, arrived.recv()).await {
            Ok(Some(Ok(item_id))) => received.push(item_id),
            Ok(Some(Err(error))) => ended = Some(error),
            Ok(None) | Err(_) => break,
        }
    }
    Cycle {
        stored,
        received,
        ended,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs nats-server -js on the path"]
async fn given_events_stored_while_the_bus_is_cut_then_each_one_comes_or_the_feed_ends_with_lost() {
    let mut world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    for round in 0..4 {
        let done = cycle(&mut world, &live).await;
        assert!(!done.stored.is_empty(), "round {round}: nothing stored");
        match done.ended {
            Some(error) => assert!(matches!(error, Error::Lost(_)), "round {round}: {error:?}"),
            None => {
                let missing: Vec<_> = done
                    .stored
                    .iter()
                    .filter(|id| !done.received.contains(id))
                    .collect();
                assert!(
                    missing.is_empty(),
                    "round {round}: lost in silence {missing:?}"
                );
            }
        }
    }
}

async fn delivered_up_to(world: &World, sequence: u64) -> bool {
    use futures::TryStreamExt;
    let stream = world.js.get_stream(INTEGRATION_EVT).await.expect("stream");
    let consumers: Vec<_> = stream
        .consumers()
        .try_collect()
        .await
        .expect("the consumers are listed");
    consumers
        .iter()
        .any(|consumer| consumer.delivered.stream_sequence >= sequence)
}

async fn pull_waiting(world: &World) -> bool {
    use futures::TryStreamExt;
    let stream = world.js.get_stream(INTEGRATION_EVT).await.expect("stream");
    let consumers: Vec<_> = stream
        .consumers()
        .try_collect()
        .await
        .expect("the consumers are listed");
    consumers.iter().any(|consumer| consumer.num_waiting > 0)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_event_written_to_a_connection_that_is_then_cut_when_the_connection_returns_then_the_event_comes()
 {
    let world = World::ready().await;
    let proxy = Proxy::start(world.server.url()).await;
    let client = async_nats::connect(proxy.url()).await.expect("a client");
    let live = Live::connect(
        crate::RunnerType::new("analyst").expect("a runner type"),
        "pod-1",
        WAIT,
        connection_on(&client, WAIT),
    )
    .await
    .map_err(|error| error.to_string())
    .expect("the live transport connects");
    let run_id = Uuid::now_v7();
    let mut feed = live
        .arrival_feed(run_id)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let (events, mut arrived) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok(item) = feed.next().await {
            if events.send(item.item_id).is_err() {
                break;
            }
        }
    });
    eventually("the feed waits for events", || pull_waiting(&world)).await;
    proxy.discard_what_the_server_sends(true);
    let in_flight = Uuid::now_v7();
    arrive(&world, run_id, in_flight, WireKind::Mail, None).await;
    eventually("the bus wrote the event to the connection", || {
        delivered_up_to(&world, 1)
    })
    .await;
    proxy.cut();
    proxy.discard_what_the_server_sends(false);
    let found = tokio::time::timeout(Duration::from_secs(40), arrived.recv())
        .await
        .expect("the event comes after the connection returns");
    assert_eq!(found, Some(in_flight));
}
