use br_util_nats_fabric::INTEGRATION_EVT;
use futures::TryStreamExt;

use super::live_world::World;

pub(super) async fn last_sequence(world: &World) -> u64 {
    let mut stream = world.js.get_stream(INTEGRATION_EVT).await.expect("stream");
    stream
        .info()
        .await
        .expect("the stream information")
        .state
        .last_sequence
}

async fn consumers(world: &World) -> Vec<async_nats::jetstream::consumer::Info> {
    let stream = world.js.get_stream(INTEGRATION_EVT).await.expect("stream");
    stream
        .consumers()
        .try_collect()
        .await
        .expect("the consumers are listed")
}

pub(super) async fn delivered_up_to(world: &World, sequence: u64) -> bool {
    consumers(world)
        .await
        .iter()
        .any(|consumer| consumer.delivered.stream_sequence >= sequence)
}

pub(super) async fn pull_waiting(world: &World) -> bool {
    consumers(world)
        .await
        .iter()
        .any(|consumer| consumer.num_waiting > 0)
}
