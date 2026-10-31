use br_util_nats_fabric::INTEGRATION_EVT;
use contract_jobs::event_v2::{ItemKind as WireKind, RunItemArrived};
use serde_json::json;
use uuid::Uuid;

use super::live_standin::publish_event;
use super::live_world::World;
use super::{live, next, silent};
use crate::backend::Backend;
use crate::error::Error;
use crate::model::{ItemArrived, ItemKind};

async fn arrive(world: &World, run_id: Uuid, item_id: Uuid, kind: WireKind, address: Option<Uuid>) {
    publish_event(
        &world.js,
        &contract_jobs::evt_run_item_arrived_v1_coords().expect("coords"),
        Uuid::now_v7(),
        RunItemArrived {
            run_id,
            address,
            item_id,
            item_kind: kind,
        },
    )
    .await;
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_open_feed_when_items_arrive_then_only_those_of_its_run_come_with_their_kind() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let (mine, other) = (Uuid::now_v7(), Uuid::now_v7());
    let mut feed = live
        .arrival_feed(mine)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let (first, second, third, address) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    arrive(&world, other, Uuid::now_v7(), WireKind::Mail, None).await;
    arrive(&world, mine, first, WireKind::Mail, Some(address)).await;
    arrive(&world, mine, second, WireKind::CcNotice, None).await;
    arrive(&world, mine, third, WireKind::Notice, None).await;
    assert_eq!(
        next(&mut feed).await,
        Ok(ItemArrived {
            address: Some(address),
            item_id: first,
            kind: ItemKind::DirectMail
        })
    );
    assert_eq!(
        next(&mut feed).await,
        Ok(ItemArrived {
            address: None,
            item_id: second,
            kind: ItemKind::CcNotice
        })
    );
    assert_eq!(
        next(&mut feed).await,
        Ok(ItemArrived {
            address: None,
            item_id: third,
            kind: ItemKind::CallNotice
        })
    );
    silent(&mut feed).await;
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_two_feeds_on_two_runs_when_items_arrive_then_each_gets_its_own_and_both_can_be_polled_together()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let (run_a, run_b) = (Uuid::now_v7(), Uuid::now_v7());
    let mut feed_a = live
        .arrival_feed(run_a)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let mut feed_b = live
        .arrival_feed(run_b)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let (item_a, item_b) = (Uuid::now_v7(), Uuid::now_v7());
    arrive(&world, run_b, item_b, WireKind::Mail, None).await;
    arrive(&world, run_a, item_a, WireKind::Mail, None).await;
    let (got_a, got_b) = tokio::join!(next(&mut feed_a), next(&mut feed_b));
    assert_eq!(got_a.map(|item| item.item_id), Ok(item_a));
    assert_eq!(got_b.map(|item| item.item_id), Ok(item_b));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_event_that_cannot_be_read_then_unreadable_comes_and_the_feed_goes_on() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    let mut feed = live
        .arrival_feed(run_id)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    publish_event(
        &world.js,
        &contract_jobs::evt_run_item_arrived_v1_coords().expect("coords"),
        Uuid::now_v7(),
        json!({ "run_id": "not a uuid" }),
    )
    .await;
    let item_id = Uuid::now_v7();
    arrive(&world, run_id, item_id, WireKind::Mail, None).await;
    assert!(matches!(next(&mut feed).await, Err(Error::Unreadable(_))));
    assert_eq!(next(&mut feed).await.map(|item| item.item_id), Ok(item_id));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_dropped_next_when_an_item_arrives_then_the_next_call_gets_it() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    let mut feed = live
        .arrival_feed(run_id)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    silent(&mut feed).await;
    let item_id = Uuid::now_v7();
    arrive(&world, run_id, item_id, WireKind::Mail, None).await;
    assert_eq!(next(&mut feed).await.map(|item| item.item_id), Ok(item_id));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_feed_dropped_when_another_opens_on_the_same_run_then_it_gets_the_new_items() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    drop(
        live.arrival_feed(run_id)
            .await
            .map_err(|e| e.to_string())
            .expect("feed"),
    );
    let mut feed = live
        .arrival_feed(run_id)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let item_id = Uuid::now_v7();
    arrive(&world, run_id, item_id, WireKind::Notice, None).await;
    assert_eq!(next(&mut feed).await.map(|item| item.item_id), Ok(item_id));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_missing_event_stream_when_opening_then_infra_missing() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    world.delete_stream(INTEGRATION_EVT).await;
    let error = live
        .arrival_feed(Uuid::now_v7())
        .await
        .err()
        .expect("refused");
    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_open_feed_when_the_event_stream_is_deleted_then_the_feed_ends_with_infra_missing()
{
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .arrival_feed(Uuid::now_v7())
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    world.delete_stream(INTEGRATION_EVT).await;
    let error = next(&mut feed).await.expect_err("ended");
    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
    let again = next(&mut feed).await.expect_err("still ended");
    assert_eq!(again, error);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_feed_waiting_with_no_arrival_when_a_second_feed_opens_then_it_opens_without_waiting()
 {
    let world = World::ready().await;
    let live = std::sync::Arc::new(live(&world, "analyst", "pod-1").await);
    let (run_a, run_b) = (Uuid::now_v7(), Uuid::now_v7());
    let mut feed_a = live
        .arrival_feed(run_a)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let waiting = tokio::spawn(async move { feed_a.next().await });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let opened =
        tokio::time::timeout(std::time::Duration::from_secs(5), live.arrival_feed(run_b)).await;
    waiting.abort();
    let mut feed_b = opened
        .expect("the second feed opened while the first one waits")
        .map_err(|e| e.to_string())
        .expect("feed");
    let item_id = Uuid::now_v7();
    arrive(&world, run_b, item_id, WireKind::Mail, None).await;
    assert_eq!(
        next(&mut feed_b).await.map(|item| item.item_id),
        Ok(item_id)
    );
}
