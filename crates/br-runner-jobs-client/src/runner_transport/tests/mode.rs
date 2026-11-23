use br_util_nats_fabric::KV_PUBLISHED_LANGUAGE;
use contract_jobs::dispatch_mode::{DispatchMode, RunnerTypeDispatchMode, dispatch_mode_key};
use contract_jobs::runner::WIRE_VERSION;

use super::live_world::World;
use super::{live, next, silent};
use crate::backend::Backend;
use crate::error::Error;
use crate::model::{DispatchMode as Mode, ModeEvent};

fn entry(runner_type: &str, mode: DispatchMode) -> Vec<u8> {
    serde_json::to_vec(&RunnerTypeDispatchMode {
        version: WIRE_VERSION,
        runner_type: runner_type.to_owned(),
        dispatch_mode: mode,
    })
    .expect("json")
}

async fn put(world: &World, runner_type: &str, body: Vec<u8>) {
    world
        .bucket(KV_PUBLISHED_LANGUAGE)
        .await
        .put(dispatch_mode_key(runner_type), body.into())
        .await
        .expect("put");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_no_entry_when_the_feed_opens_then_no_entry_comes_first() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .mode_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::NoEntry));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_stored_entry_when_the_feed_opens_then_the_mode_comes_first_and_changes_follow() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    put(&world, "analyst", entry("analyst", DispatchMode::Claim)).await;
    let mut feed = live
        .mode_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::Mode(Mode::Claim)));
    put(&world, "analyst", entry("analyst", DispatchMode::Push)).await;
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::Mode(Mode::Push)));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_mode_when_the_entry_is_deleted_then_no_entry_comes() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    put(&world, "analyst", entry("analyst", DispatchMode::Push)).await;
    let mut feed = live
        .mode_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::Mode(Mode::Push)));
    world
        .bucket(KV_PUBLISHED_LANGUAGE)
        .await
        .delete(dispatch_mode_key("analyst"))
        .await
        .expect("delete");
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::NoEntry));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_the_entry_of_another_runner_type_when_it_changes_then_this_feed_stays_silent() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .mode_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::NoEntry));
    put(
        &world,
        "analyst-two",
        entry("analyst-two", DispatchMode::Claim),
    )
    .await;
    silent(&mut feed).await;
    put(&world, "analyst", entry("analyst", DispatchMode::Claim)).await;
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::Mode(Mode::Claim)));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_the_same_mode_put_again_then_no_second_event_comes() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .mode_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::NoEntry));
    put(&world, "analyst", entry("analyst", DispatchMode::Claim)).await;
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::Mode(Mode::Claim)));
    put(&world, "analyst", entry("analyst", DispatchMode::Claim)).await;
    silent(&mut feed).await;
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_entry_that_cannot_be_read_when_stored_then_unreadable() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .mode_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::NoEntry));
    put(
        &world,
        "analyst",
        br#"{"version":1,"runner_type":"analyst"}"#.to_vec(),
    )
    .await;
    assert!(matches!(next(&mut feed).await, Err(Error::Unreadable(_))));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_unreadable_entry_at_open_then_unreadable_comes_first_and_a_good_one_later_is_read()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    put(
        &world,
        "analyst",
        br#"{"runner_type":"analyst","dispatch_mode":"SOMETIMES"}"#.to_vec(),
    )
    .await;
    let mut feed = live
        .mode_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert!(matches!(next(&mut feed).await, Err(Error::Unreadable(_))));
    put(&world, "analyst", entry("analyst", DispatchMode::Push)).await;
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::Mode(Mode::Push)));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_missing_published_language_bucket_when_opening_then_infra_missing() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    world.delete_bucket(KV_PUBLISHED_LANGUAGE).await;
    let error = live.mode_feed().await.err().expect("refused");
    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_mode_then_an_unreadable_entry_then_the_same_mode_again_then_the_recovery_is_given()
{
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .mode_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::NoEntry));
    put(&world, "analyst", entry("analyst", DispatchMode::Claim)).await;
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::Mode(Mode::Claim)));
    put(
        &world,
        "analyst",
        br#"{"runner_type":"analyst","dispatch_mode":"SOMETIMES"}"#.to_vec(),
    )
    .await;
    assert!(matches!(next(&mut feed).await, Err(Error::Unreadable(_))));
    put(&world, "analyst", entry("analyst", DispatchMode::Claim)).await;
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::Mode(Mode::Claim)));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_entry_that_is_not_json_then_unreadable_comes_and_the_fabric_watch_ends_the_feed_with_lost()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .mode_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::NoEntry));
    put(&world, "analyst", b"{".to_vec()).await;
    assert!(matches!(next(&mut feed).await, Err(Error::Unreadable(_))));
    assert!(matches!(next(&mut feed).await, Err(Error::Lost(_))));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_open_feed_when_the_bucket_is_deleted_then_the_feed_ends_with_a_typed_error() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .mode_feed()
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    assert_eq!(next(&mut feed).await, Ok(ModeEvent::NoEntry));
    world.delete_bucket(KV_PUBLISHED_LANGUAGE).await;
    let error = tokio::time::timeout(std::time::Duration::from_secs(20), feed.next())
        .await
        .expect("the feed ends")
        .expect_err("ended");
    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
}
