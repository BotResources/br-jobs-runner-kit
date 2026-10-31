mod support;

use std::time::Duration;

use br_core_integration::IntegrationCommand;
use br_runner_threads::{Cause, Error};
use serde_json::Value;
use support::nats_fixtures::*;
use support::nats_responder::{Reply, Responder};
use support::nats_server::Server;
use support::nats_streams::*;

async fn infra() -> (Server, async_nats::Client) {
    let server = Server::start();
    let bus = server.client().await;
    create_all_streams(&bus).await;
    (server, bus)
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_command_after_its_stream_was_deleted_is_unavailable() {
    let (_server, bus) = infra().await;
    let client = built(&bus).await;
    delete_stream(&bus, COMMANDS).await;

    let result = client.create_thread(&new_thread()).await;

    assert!(matches!(result, Err(Error::Unavailable(_))), "{result:?}");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_command_after_the_event_stream_was_deleted_is_unavailable_and_publishes_nothing() {
    let (_server, bus) = infra().await;
    let client = built(&bus).await;
    delete_stream(&bus, EVENTS).await;

    let result = client.create_thread(&new_thread()).await;

    assert!(matches!(result, Err(Error::Unavailable(_))), "{result:?}");
    assert_eq!(stored_count(&bus, COMMANDS).await, 0);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_command_after_the_server_went_away_is_unavailable_within_the_wait() {
    let (server, bus) = infra().await;
    let client = built(&bus).await;
    let mut request = new_thread();
    request.wait = Duration::from_secs(1);
    drop(server);

    let result = tokio::time::timeout(Duration::from_secs(5), client.create_thread(&request))
        .await
        .expect("the command ended within its wait");

    assert!(matches!(result, Err(Error::Unavailable(_))), "{result:?}");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_resend_under_the_same_command_id_reaches_threads_each_time() {
    let (_server, bus) = infra().await;
    let responder = Responder::start(&bus).await;
    let client = built(&bus).await;
    let mut request = new_thread();
    request.wait = Duration::from_millis(300);
    responder.script(request.command_id, [Reply::Silence]);

    let first = client.create_thread(&request).await;
    request.wait = WAIT;
    let second = client.create_thread(&request).await.expect("confirmed");

    assert!(matches!(first, Err(Error::Lost(ref failure)) if failure.cause == Cause::WaitFired));
    assert_eq!(second.command_id, request.command_id);
    let commands = stored(&bus, COMMANDS).await;
    assert_eq!(commands.len(), 2);
    let payloads: Vec<Value> = commands
        .iter()
        .map(|c| c.json::<IntegrationCommand<Value>>().payload)
        .collect();
    assert_eq!(payloads[0], payloads[1]);
    assert!(
        commands
            .iter()
            .all(|c| c.json::<IntegrationCommand<Value>>().command_id == request.command_id)
    );
}
