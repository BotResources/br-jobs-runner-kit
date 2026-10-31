use contract_jobs::runner::{CANCEL_BUCKET, CancelRun, WIRE_VERSION};
use contract_jobs::runner_transport::run_cancel_key;
use uuid::Uuid;

use super::live;
use super::live_world::World;
use super::{next, silent};
use crate::backend::Backend;
use crate::error::Error;
use crate::model::CancelEvent;

fn order(run_id: Uuid) -> Vec<u8> {
    serde_json::to_vec(&CancelRun {
        version: WIRE_VERSION,
        run_id,
    })
    .expect("json")
}

async fn put(world: &World, run_id: Uuid) {
    world
        .bucket(CANCEL_BUCKET)
        .await
        .put(run_cancel_key(run_id), order(run_id).into())
        .await
        .expect("put");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_empty_bucket_when_the_feed_opens_then_replay_complete_comes_once_and_nothing_else()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .cancel_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::ReplayComplete));
    silent(&mut feed).await;
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_stored_orders_when_the_feed_opens_then_each_comes_then_replay_complete_once() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let (first, second) = (Uuid::now_v7(), Uuid::now_v7());
    put(&world, first).await;
    put(&world, second).await;
    let mut feed = live
        .cancel_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let mut made = vec![next(&mut feed).await, next(&mut feed).await];
    made.sort_by_key(|event| format!("{event:?}"));
    let mut wanted = vec![
        Ok(CancelEvent::OrderMade { run_id: first }),
        Ok(CancelEvent::OrderMade { run_id: second }),
    ];
    wanted.sort_by_key(|event| format!("{event:?}"));
    assert_eq!(made, wanted);
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::ReplayComplete));
    silent(&mut feed).await;
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_replayed_feed_when_an_order_is_put_then_order_made_and_when_deleted_then_order_deleted()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .cancel_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::ReplayComplete));
    let run_id = Uuid::now_v7();
    put(&world, run_id).await;
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::OrderMade { run_id }));
    world
        .bucket(CANCEL_BUCKET)
        .await
        .delete(run_cancel_key(run_id))
        .await
        .expect("delete");
    assert_eq!(
        next(&mut feed).await,
        Ok(CancelEvent::OrderDeleted { run_id })
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_order_deleted_before_the_feed_opens_then_no_event_comes_for_it() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let gone = Uuid::now_v7();
    put(&world, gone).await;
    world
        .bucket(CANCEL_BUCKET)
        .await
        .delete(run_cancel_key(gone))
        .await
        .expect("delete");
    let mut feed = live
        .cancel_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::ReplayComplete));
    silent(&mut feed).await;
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_key_that_is_not_a_run_id_when_it_is_put_then_it_is_ignored() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .cancel_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::ReplayComplete));
    world
        .bucket(CANCEL_BUCKET)
        .await
        .put("not-a-run", b"x".to_vec().into())
        .await
        .expect("put");
    let run_id = Uuid::now_v7();
    put(&world, run_id).await;
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::OrderMade { run_id }));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_order_that_cannot_be_read_when_put_then_unreadable_and_the_feed_goes_on() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .cancel_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::ReplayComplete));
    let (broken, other, good) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let bucket = world.bucket(CANCEL_BUCKET).await;
    bucket
        .put(run_cancel_key(broken), b"{".to_vec().into())
        .await
        .expect("put");
    bucket
        .put(run_cancel_key(other), order(good).into())
        .await
        .expect("put");
    put(&world, good).await;
    assert!(matches!(next(&mut feed).await, Err(Error::Unreadable(_))));
    assert!(matches!(next(&mut feed).await, Err(Error::Unreadable(_))));
    assert_eq!(
        next(&mut feed).await,
        Ok(CancelEvent::OrderMade { run_id: good })
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_next_that_was_dropped_when_an_order_comes_then_the_next_call_gets_it() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .cancel_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::ReplayComplete));
    drop(tokio::time::timeout(std::time::Duration::from_millis(100), feed.next()).await);
    let run_id = Uuid::now_v7();
    put(&world, run_id).await;
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::OrderMade { run_id }));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_missing_bucket_when_opening_the_feed_then_a_typed_error() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    world.delete_bucket(CANCEL_BUCKET).await;
    let error = live.cancel_feed().await.err().expect("refused");
    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_open_feed_when_the_bucket_is_deleted_then_the_feed_ends_with_a_typed_error() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .cancel_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(CancelEvent::ReplayComplete));
    world.delete_bucket(CANCEL_BUCKET).await;
    let error = tokio::time::timeout(std::time::Duration::from_secs(30), feed.next())
        .await
        .expect("the feed ends")
        .expect_err("ended");
    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
}
