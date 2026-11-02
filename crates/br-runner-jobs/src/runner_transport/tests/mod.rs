mod arrivals;
mod bind;
mod cancel;
mod claim;
mod commands;
mod commands_outcomes;
mod commands_session;
mod cut;
mod facts;
mod listing;
mod mode;
mod presence;
mod surface;
mod triggers;

#[allow(dead_code)] // the support is shared with the integration tests and each file uses part of it
#[path = "../../../tests/support/live_server.rs"]
mod live_server;
#[allow(dead_code)] // the support is shared with the integration tests and each file uses part of it
#[path = "../../../tests/support/live_standin.rs"]
mod live_standin;
#[allow(dead_code)] // the support is shared with the integration tests and each file uses part of it
#[path = "../../../tests/support/live_world.rs"]
mod live_world;

use std::time::Duration;

use super::Live;
use crate::backend::RawFeed;
use crate::error::Error;
use live_world::{WAIT, World, connection_on};

pub(super) async fn live(world: &World, runner_type: &str, instance_key: &str) -> Live {
    Live::connect(
        crate::RunnerType::new(runner_type).expect("a runner type"),
        instance_key,
        WAIT,
        world.connection(),
    )
    .await
    .map_err(|error| error.to_string())
    .expect("the live transport connects")
}

pub(super) async fn live_with_wait(
    world: &World,
    runner_type: &str,
    instance_key: &str,
    wait: Duration,
) -> Live {
    Live::connect(
        crate::RunnerType::new(runner_type).expect("a runner type"),
        instance_key,
        wait,
        connection_on(&world.client, wait),
    )
    .await
    .map_err(|error| error.to_string())
    .expect("the live transport connects")
}

pub(super) async fn refused_connect(
    world: &World,
    runner_type: &str,
    instance_key: &str,
    wait: Duration,
) -> Error {
    match Live::connect(
        crate::RunnerType::new(runner_type).expect("a runner type"),
        instance_key,
        wait,
        connection_on(&world.client, WAIT),
    )
    .await
    {
        Err(error) => error,
        Ok(_) => panic!("the live transport connected"),
    }
}

pub(super) async fn next<T>(feed: &mut Box<dyn RawFeed<T>>) -> Result<T, Error> {
    tokio::time::timeout(Duration::from_secs(10), feed.next())
        .await
        .expect("the feed gave an item in time")
}

pub(super) async fn silent<T: std::fmt::Debug>(feed: &mut Box<dyn RawFeed<T>>) {
    let waited = tokio::time::timeout(Duration::from_millis(300), feed.next()).await;
    assert!(waited.is_err(), "the feed gave {waited:?}");
}
