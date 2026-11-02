use br_util_nats_fabric::INTEGRATION_EVT;
use contract_jobs::event_v2::ItemKind as WireKind;
use uuid::Uuid;

use super::arrivals::arrive;
use super::live_world::World;
use super::{arrival_settings, live, next, silent};
use crate::arrival::ArrivalSettings;
use crate::backend::Backend;
use crate::error::Error;

async fn consumers_of(world: &World, stream: &str) -> Vec<async_nats::jetstream::consumer::Info> {
    use futures::TryStreamExt;
    let stream = world.js.get_stream(stream).await.expect("the stream");
    stream
        .consumers()
        .try_collect()
        .await
        .expect("the consumers are listed")
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_feeds_opened_and_read_then_every_consumer_of_the_event_stream_is_ephemeral_and_starts_at_new_messages()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    arrive(&world, run_id, Uuid::now_v7(), WireKind::Mail, None).await;
    let mut first = live
        .arrival_feed(run_id, arrival_settings())
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let mut second = live
        .arrival_feed(run_id, arrival_settings())
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    silent(&mut first).await;
    let item_id = Uuid::now_v7();
    arrive(&world, run_id, item_id, WireKind::Mail, None).await;
    assert_eq!(next(&mut first).await.map(|i| i.item_id), Ok(item_id));
    assert_eq!(next(&mut second).await.map(|i| i.item_id), Ok(item_id));
    let consumers = consumers_of(&world, INTEGRATION_EVT).await;
    assert_eq!(consumers.len(), 2);
    for consumer in consumers {
        assert_eq!(consumer.config.durable_name, None);
        assert_eq!(
            consumer.config.deliver_policy,
            async_nats::jetstream::consumer::DeliverPolicy::New
        );
        assert_eq!(
            consumer.config.ack_policy,
            async_nats::jetstream::consumer::AckPolicy::Explicit
        );
        assert_eq!(consumer.config.ack_wait, arrival_settings().ack_wait);
        assert_eq!(
            consumer.config.inactive_threshold,
            arrival_settings().inactivity_limit
        );
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_feed_that_read_events_then_the_server_counts_each_one_as_acknowledged() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    let mut feed = live
        .arrival_feed(run_id, arrival_settings())
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    silent(&mut feed).await;
    for _ in 0..3 {
        let item_id = Uuid::now_v7();
        arrive(&world, run_id, item_id, WireKind::Mail, None).await;
        assert_eq!(next(&mut feed).await.map(|i| i.item_id), Ok(item_id));
    }
    super::live_world::eventually("the three events are acknowledged", || async {
        consumers_of(&world, INTEGRATION_EVT)
            .await
            .iter()
            .all(|consumer| {
                consumer.ack_floor.stream_sequence == 3 && consumer.num_ack_pending == 0
            })
    })
    .await;
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_feed_that_is_not_polled_when_items_arrive_then_all_of_them_come_in_order_later() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let run_id = Uuid::now_v7();
    let mut feed = live
        .arrival_feed(run_id, arrival_settings())
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let ids: Vec<Uuid> = (0..5).map(|_| Uuid::now_v7()).collect();
    for id in &ids {
        arrive(&world, run_id, *id, WireKind::Mail, None).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    for id in &ids {
        assert_eq!(next(&mut feed).await.map(|i| i.item_id), Ok(*id));
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_an_open_feed_when_its_consumer_is_deleted_then_the_feed_ends_with_lost_and_stays_ended()
 {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let mut feed = live
        .arrival_feed(Uuid::now_v7(), arrival_settings())
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let stream = world.js.get_stream(INTEGRATION_EVT).await.expect("stream");
    for consumer in consumers_of(&world, INTEGRATION_EVT).await {
        stream
            .delete_consumer(&consumer.name)
            .await
            .expect("deleted");
    }
    let error = next(&mut feed).await.expect_err("ended");
    assert!(matches!(error, Error::Lost(_)), "{error:?}");
    assert_eq!(next(&mut feed).await.expect_err("still ended"), error);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_triggers_feeds_and_commands_in_use_then_the_only_durable_consumer_is_the_trigger_one()
 {
    use super::commands::{commands_on, create_command};
    use super::live_standin::publish_event;
    use crate::backend::{Backend, ChildCommand};
    use contract_jobs::event_v2::{JobQueuedV2, RequesterRef};
    use contract_jobs::runner::TRIGGER_STREAM;
    use futures::TryStreamExt;

    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let _triggers = live
        .open_triggers(std::time::Duration::from_secs(30))
        .await
        .map_err(|e| e.to_string())
        .expect("source");
    let run_id = Uuid::now_v7();
    let _feed = live
        .arrival_feed(run_id, arrival_settings())
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let mut seen = commands_on(
        &world,
        contract_jobs::cmd_job_create_v3_coords().expect("coords"),
    )
    .await;
    let command = create_command(Uuid::now_v7(), run_id);
    let (command_id, job_id, parent, initial) = (
        command.command_id,
        command.job_id,
        command.parent_job_id,
        command.initial_job_id,
    );
    let service = async {
        seen.request().await;
        publish_event(
            &world.js,
            &contract_jobs::evt_job_queued_v2_coords().expect("coords"),
            command_id,
            JobQueuedV2 {
                job_id,
                parent_job_id: parent,
                initial_job_id: initial,
                runner_type: "writer".to_owned(),
                requester: RequesterRef::Run { run_id },
                restart_of_job_id: None,
                retry_of_job_id: None,
            },
        )
        .await;
    };
    let (result, ()) = tokio::join!(
        live.child_command(ChildCommand::Create(command), super::live_world::WAIT),
        service
    );
    result.map_err(|e| e.to_string()).expect("accepted");
    let mut durables = Vec::new();
    for name in [
        INTEGRATION_EVT,
        br_util_nats_fabric::INTEGRATION_CMD,
        TRIGGER_STREAM,
    ] {
        let stream = world.js.get_stream(name).await.expect("the stream");
        let consumers: Vec<_> = stream.consumers().try_collect().await.expect("listed");
        durables.extend(
            consumers
                .into_iter()
                .filter_map(|consumer| consumer.config.durable_name.map(|d| (name, d))),
        );
    }
    assert_eq!(durables.len(), 1, "{durables:?}");
    assert_eq!(durables[0].0, TRIGGER_STREAM);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_the_importer_values_when_a_feed_is_opened_then_the_consumer_carries_them() {
    let world = World::ready().await;
    let live = live(&world, "analyst", "pod-1").await;
    let settings = ArrivalSettings {
        ack_wait: std::time::Duration::from_secs(7),
        inactivity_limit: std::time::Duration::from_secs(90),
    };
    let _feed = live
        .arrival_feed(Uuid::now_v7(), settings)
        .await
        .map_err(|e| e.to_string())
        .expect("feed");
    let consumers = consumers_of(&world, INTEGRATION_EVT).await;
    assert_eq!(consumers.len(), 1);
    assert_eq!(
        consumers[0].config.ack_wait,
        std::time::Duration::from_secs(7)
    );
    assert_eq!(
        consumers[0].config.inactive_threshold,
        std::time::Duration::from_secs(90)
    );
}
