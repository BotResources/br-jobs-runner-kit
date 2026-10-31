mod support;

use std::time::Duration;

use br_runner_threads::{Cause, Error};
use support::nats_fixtures::*;
use support::nats_server::Server;
use support::nats_streams::*;

async fn without(missing: &str) -> (Server, async_nats::Client, Error) {
    let server = Server::start();
    let bus = server.client().await;
    create_streams_except(&bus, missing).await;
    let error = build_error(&bus, CHECK_WAIT).await;
    (server, bus, error)
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn building_without_the_command_stream_is_infra_missing_and_creates_nothing() {
    let (_server, bus, error) = without(COMMANDS).await;

    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
    assert!(!stream_exists(&bus, COMMANDS).await);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn building_without_the_event_stream_is_infra_missing_and_creates_nothing() {
    let (_server, bus, error) = without(EVENTS).await;

    assert!(matches!(error, Error::InfraMissing(_)), "{error:?}");
    assert!(!stream_exists(&bus, EVENTS).await);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn building_without_the_chunk_stream_is_infra_missing_and_names_it() {
    let (_server, bus, error) = without(CHUNKS).await;

    match error {
        Error::InfraMissing(text) => assert!(text.contains(CHUNKS), "{text}"),
        other => panic!("InfraMissing, not {other:?}"),
    }
    assert!(!stream_exists(&bus, CHUNKS).await);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn building_over_a_bus_that_never_connected_is_unavailable_within_the_check_wait() {
    let bus = idle_bus().await;

    let error = tokio::time::timeout(
        Duration::from_secs(4),
        build_error(&bus, Duration::from_millis(300)),
    )
    .await
    .expect("the check ended within its wait");

    match error {
        Error::Unavailable(failure) => assert_eq!(failure.cause, Cause::WaitFired),
        other => panic!("Unavailable, not {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn building_after_the_server_went_away_is_unavailable() {
    let server = Server::start();
    let bus = server.client().await;
    create_all_streams(&bus).await;
    drop(server);

    let error = tokio::time::timeout(
        Duration::from_secs(8),
        build_error(&bus, Duration::from_millis(500)),
    )
    .await
    .expect("the check ended");

    assert!(matches!(error, Error::Unavailable(_)), "{error:?}");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn building_publishes_nothing_and_creates_no_stream() {
    let server = Server::start();
    let bus = server.client().await;
    create_all_streams(&bus).await;

    let _client = built(&bus).await;

    for name in [COMMANDS, EVENTS, CHUNKS] {
        assert_eq!(stored_count(&bus, name).await, 0, "{name}");
        assert!(stream_exists(&bus, name).await);
    }
}
