use std::time::Duration;

use async_nats::jetstream::consumer::{AckPolicy, DeliverPolicy};
use contract_jobs::runner::{TRIGGER_STREAM, TriggerV2};
use contract_jobs::runner_transport::trigger_subject;
use contract_jobs::segment::SubjectSegment;
use uuid::Uuid;

use super::live;
use super::live_standin::{publish_trigger, trigger};
use super::live_world::World;
use crate::backend::{Backend, IncomingTrigger, TriggerSource};
use crate::error::Error;
use crate::runner_transport::triggers::DURABLE_PREFIX;

const ACK_WAIT: Duration = Duration::from_secs(30);

async fn next(source: &mut Box<dyn TriggerSource>) -> IncomingTrigger {
    tokio::time::timeout(Duration::from_secs(10), source.next())
        .await
        .expect("a trigger came")
        .map_err(|e| e.to_string())
        .expect("the trigger is read")
}

async fn nothing_comes(source: &mut Box<dyn TriggerSource>) {
    assert!(
        tokio::time::timeout(Duration::from_millis(400), source.next())
            .await
            .is_err(),
        "a trigger came"
    );
}

fn decoded(incoming: &IncomingTrigger) -> TriggerV2 {
    serde_json::from_slice(&incoming.raw).expect("the raw trigger is a TriggerV2")
}

async fn consumer_info(world: &World, runner_type: &str) -> async_nats::jetstream::consumer::Info {
    let stream = world.js.get_stream(TRIGGER_STREAM).await.expect("stream");
    let mut consumer: async_nats::jetstream::consumer::PullConsumer = stream
        .get_consumer(&format!("{DURABLE_PREFIX}{runner_type}"))
        .await
        .expect("the durable");
    consumer.info().await.expect("info").clone()
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_published_trigger_when_read_then_the_raw_bytes_come_and_ack_removes_it_when_the_broker_confirmed()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut source = live
        .open_triggers(ACK_WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    let sent = trigger(Uuid::now_v7(), Uuid::now_v7());
    publish_trigger(&world.js, "analyst", &sent).await;
    let incoming = next(&mut source).await;
    assert_eq!(decoded(&incoming), sent);
    assert_eq!(world.messages_in(TRIGGER_STREAM).await, 1);
    incoming.delivery.ack().await.expect("acknowledged");
    assert_eq!(world.messages_in(TRIGGER_STREAM).await, 0);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_trigger_given_back_when_read_again_then_it_comes_again() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut source = live
        .open_triggers(ACK_WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    let sent = trigger(Uuid::now_v7(), Uuid::now_v7());
    publish_trigger(&world.js, "analyst", &sent).await;
    let first = next(&mut source).await;
    first.delivery.give_back().await.expect("given back");
    let second = next(&mut source).await;
    assert_eq!(decoded(&second), sent);
    second.delivery.ack().await.expect("acknowledged");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_trigger_terminated_when_waiting_then_it_never_comes_again() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut source = live
        .open_triggers(Duration::from_secs(1))
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    publish_trigger(
        &world.js,
        "analyst",
        &trigger(Uuid::now_v7(), Uuid::now_v7()),
    )
    .await;
    let incoming = next(&mut source).await;
    incoming.delivery.terminate().await.expect("terminated");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    nothing_comes(&mut source).await;
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_trigger_of_another_runner_type_when_reading_then_only_the_own_ones_come() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut source = live
        .open_triggers(ACK_WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    publish_trigger(&world.js, "other", &trigger(Uuid::now_v7(), Uuid::now_v7())).await;
    nothing_comes(&mut source).await;
    let own = trigger(Uuid::now_v7(), Uuid::now_v7());
    publish_trigger(&world.js, "analyst", &own).await;
    assert_eq!(decoded(&next(&mut source).await), own);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_three_triggers_when_the_importer_has_not_asked_then_none_is_taken_and_one_ask_takes_one()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut source = live
        .open_triggers(ACK_WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    for _ in 0..3 {
        publish_trigger(
            &world.js,
            "analyst",
            &trigger(Uuid::now_v7(), Uuid::now_v7()),
        )
        .await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(consumer_info(&world, "analyst").await.num_ack_pending, 0);
    let _first = next(&mut source).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let info = consumer_info(&world, "analyst").await;
    assert_eq!(info.num_ack_pending, 1);
    assert_eq!(info.num_pending, 2);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_next_that_was_dropped_when_a_trigger_comes_then_the_next_call_gets_it() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut source = live
        .open_triggers(ACK_WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    nothing_comes(&mut source).await;
    nothing_comes(&mut source).await;
    let sent = trigger(Uuid::now_v7(), Uuid::now_v7());
    publish_trigger(&world.js, "analyst", &sent).await;
    assert_eq!(decoded(&next(&mut source).await), sent);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_two_instances_when_each_asks_then_they_compete_on_one_durable_and_never_share_a_trigger()
 {
    let world = World::ready().await;
    let first = live(&world, "analyst", "pod-1").await;
    let second = live(&world, "analyst", "pod-2").await;
    let mut a = first
        .open_triggers(ACK_WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    let mut b = second
        .open_triggers(ACK_WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    let (one, two) = (
        trigger(Uuid::now_v7(), Uuid::now_v7()),
        trigger(Uuid::now_v7(), Uuid::now_v7()),
    );
    publish_trigger(&world.js, "analyst", &one).await;
    publish_trigger(&world.js, "analyst", &two).await;
    let got_a = decoded(&next(&mut a).await);
    let got_b = decoded(&next(&mut b).await);
    assert_ne!(got_a.run_id, got_b.run_id);
    let stream = world.js.get_stream(TRIGGER_STREAM).await.expect("stream");
    assert_eq!(stream.cached_info().state.consumer_count, 1);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_durable_with_another_configuration_when_opening_then_it_converges_to_the_stated_one()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let segment = SubjectSegment::runner_type("analyst").expect("segment");
    let stream = world.js.get_stream(TRIGGER_STREAM).await.expect("stream");
    stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some(format!("{DURABLE_PREFIX}analyst")),
            filter_subject: trigger_subject(&segment),
            ack_policy: AckPolicy::Explicit,
            ack_wait: Duration::from_secs(3),
            max_ack_pending: 7,
            max_deliver: 4,
            ..Default::default()
        })
        .await
        .expect("a durable that gitops or an older runner left");
    let _source = live
        .open_triggers(Duration::from_secs(11))
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    let config = consumer_info(&world, "analyst").await.config;
    assert_eq!(config.ack_wait, Duration::from_secs(11));
    assert_eq!(config.max_ack_pending, -1);
    assert_eq!(config.max_deliver, -1);
    assert_eq!(
        config.filter_subject,
        contract_jobs::runner_transport::trigger_v2_subject(&segment)
    );
    assert!(matches!(config.ack_policy, AckPolicy::Explicit));
    assert!(matches!(config.deliver_policy, DeliverPolicy::All));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_second_open_when_the_ack_wait_differs_then_the_last_ack_wait_applies() {
    let world = World::ready().await;
    let first = live(&world, "analyst", "pod-1").await;
    let second = live(&world, "analyst", "pod-2").await;
    let _a = first
        .open_triggers(Duration::from_secs(20))
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    let _b = second
        .open_triggers(Duration::from_secs(40))
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    assert_eq!(
        consumer_info(&world, "analyst").await.config.ack_wait,
        Duration::from_secs(40)
    );
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_missing_trigger_stream_when_opening_then_infra_missing() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    world.delete_stream(TRIGGER_STREAM).await;
    match live.open_triggers(ACK_WAIT).await {
        Err(Error::InfraMissing(text)) => assert!(text.contains(TRIGGER_STREAM)),
        Err(other) => panic!("{other:?}"),
        Ok(_) => panic!("opened"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_open_source_when_the_stream_is_deleted_then_next_ends_with_a_typed_error() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut source = live
        .open_triggers(ACK_WAIT)
        .await
        .map_err(|e| e.to_string())
        .expect("opened");
    nothing_comes(&mut source).await;
    world.delete_stream(TRIGGER_STREAM).await;
    let result = tokio::time::timeout(Duration::from_secs(20), source.next())
        .await
        .expect("an answer");
    match result {
        Err(Error::InfraMissing(_)) => {}
        Err(other) => panic!("{other:?}"),
        Ok(_) => panic!("a trigger came"),
    }
}
