use contract_jobs::runner::{PRESENCE_BUCKET, Presence, RunnerStatus};
use contract_jobs::runner_transport::runner_presence_key;
use contract_jobs::segment::SubjectSegment;

use super::live;
use super::live_world::{PRESENCE_TTL, World};
use crate::backend::Backend;
use crate::error::Error;
use crate::model::PresenceStatus;

fn key(runner_type: &str, instance_key: &str) -> String {
    runner_presence_key(
        &SubjectSegment::runner_type(runner_type).expect("segment"),
        &SubjectSegment::instance_key(instance_key).expect("segment"),
    )
}

async fn stored(world: &World, key: &str) -> Option<Presence> {
    let bucket = world.presence_bucket().await;
    bucket
        .get(key)
        .await
        .expect("the read")
        .map(|bytes| serde_json::from_slice(&bytes).expect("a Presence of the contract"))
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_presence_when_put_then_the_bucket_holds_the_wire_value_under_the_wire_key() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    live.presence_put("1.4.2".to_owned(), PresenceStatus::Ready, 3)
        .await
        .map_err(|e| e.to_string())
        .expect("put");
    let presence = stored(&world, &key("analyst", "pod-1"))
        .await
        .expect("an entry");
    assert_eq!(presence.runner_type, "analyst");
    assert_eq!(presence.instance_key, "pod-1");
    assert_eq!(presence.runner_version, "1.4.2");
    assert_eq!(presence.status, RunnerStatus::Ready);
    assert_eq!(presence.capacity.get(), 3);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_presence_when_put_again_then_the_value_is_replaced_and_delete_removes_it() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    live.presence_put("1".to_owned(), PresenceStatus::Ready, 2)
        .await
        .map_err(|e| e.to_string())
        .expect("put");
    live.presence_put("1".to_owned(), PresenceStatus::Draining, 2)
        .await
        .map_err(|e| e.to_string())
        .expect("put");
    let presence = stored(&world, &key("analyst", "pod-1"))
        .await
        .expect("an entry");
    assert_eq!(presence.status, RunnerStatus::Draining);
    live.presence_delete()
        .await
        .map_err(|e| e.to_string())
        .expect("deleted");
    assert!(stored(&world, &key("analyst", "pod-1")).await.is_none());
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_two_instances_when_one_deletes_then_the_other_presence_stays() {
    let world = World::ready().await;
    let first = live(&world, "analyst", "pod-1").await;
    let second = live(&world, "analyst", "pod-2").await;
    for live in [&first, &second] {
        live.presence_put("1".to_owned(), PresenceStatus::Ready, 1)
            .await
            .map_err(|e| e.to_string())
            .expect("put");
    }
    first
        .presence_delete()
        .await
        .map_err(|e| e.to_string())
        .expect("deleted");
    assert!(stored(&world, &key("analyst", "pod-1")).await.is_none());
    assert!(stored(&world, &key("analyst", "pod-2")).await.is_some());
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_the_bucket_when_reading_the_time_to_live_then_it_is_the_max_age_of_the_bucket() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    assert_eq!(
        live.presence_ttl()
            .await
            .map_err(|e| e.to_string())
            .expect("ttl"),
        PRESENCE_TTL
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_capacity_of_zero_when_put_then_invalid_input_and_nothing_is_stored() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let error = live
        .presence_put("1".to_owned(), PresenceStatus::Ready, 0)
        .await
        .expect_err("refused");
    assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
    assert!(stored(&world, &key("analyst", "pod-1")).await.is_none());
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_the_bucket_gone_when_put_then_a_typed_error_is_returned() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    world.delete_bucket(PRESENCE_BUCKET).await;
    let error = live
        .presence_put("1".to_owned(), PresenceStatus::Ready, 1)
        .await
        .expect_err("refused");
    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
}
