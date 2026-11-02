use std::time::Duration;

use contract_jobs::event_v2::{ItemKindV1 as WireKind, RunItemArrived};
use contract_jobs::runner::{CANCEL_BUCKET, TRIGGER_STREAM};
use contract_jobs::runner_transport::run_cancel_key;
use uuid::Uuid;

use super::live;
use super::live_standin::{publish_event, publish_trigger, trigger};
use super::live_world::{World, eventually};
use super::next;
use crate::backend::Backend;
use crate::error::Error;
use crate::model::CancelEvent;

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
async fn given_an_arrival_feed_when_the_bus_is_cut_then_it_goes_on_without_loss_or_ends_with_a_typed_error()
 {
    let mut world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    let mut feed = live
        .arrival_feed(run_id)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    cut(&mut world).await;
    let item_id = Uuid::now_v7();
    publish_event(
        &world.js,
        &contract_jobs::evt_run_item_arrived_v1_coords().expect("coords"),
        Uuid::now_v7(),
        RunItemArrived {
            run_id,
            address: None,
            item_id,
            item_kind: WireKind::Mail,
        },
    )
    .await;
    let outcome = next(&mut feed).await;
    survives_or_lost(&outcome);
    if let Ok(item) = outcome {
        assert_eq!(item.item_id, item_id);
    }
}
