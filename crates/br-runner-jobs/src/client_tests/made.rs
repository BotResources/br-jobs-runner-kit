use std::time::Duration;

use br_core_kernel::ServiceAccountId;
use br_runner_connection::{Connection, FixedToken};

use super::*;
use crate::error::Error;

const OFFLINE: &str = "http://127.0.0.1:9/graphql";

async fn offline_connection() -> Connection {
    let bus = async_nats::ConnectOptions::new()
        .retry_on_initial_connect()
        .connect("nats://127.0.0.1:1")
        .await
        .expect("a client that retries");
    Connection::new(
        OFFLINE,
        Arc::new(FixedToken::new("a-token-for-the-made-test")),
        bus,
        ServiceAccountId(id(90)),
        Duration::from_secs(1),
    )
    .expect("a connection")
}

async fn made(runner_type: &str, instance_key: &str, wait: Duration) -> Result<JobsClient, Error> {
    JobsClient::new(
        crate::RunnerType::new(runner_type)?,
        instance_key,
        wait,
        offline_connection().await,
    )
    .await
}

#[tokio::test]
async fn given_a_runner_type_or_an_instance_key_the_wire_does_not_accept_when_the_client_is_made_then_invalid_input_that_does_not_echo_the_value()
 {
    let bad = [
        "",
        "a.b",
        "a b",
        "a*",
        "a>",
        "*",
        ">",
        ".",
        "-a",
        "a\tb",
        "a\nb",
        "é",
        &"x".repeat(65),
    ];
    for value in bad {
        for (runner_type, instance_key) in [(value, "pod-1"), ("analyst", value)] {
            match made(runner_type, instance_key, Duration::from_secs(1)).await {
                Err(Error::InvalidInput(text)) => {
                    if value.len() > 1 {
                        assert!(!text.contains(value), "{text}");
                    }
                }
                other => panic!("{value:?}: {:?}", other.map(|_| ())),
            }
        }
    }
}

#[tokio::test]
async fn given_a_zero_reply_wait_when_the_client_is_made_then_invalid_input() {
    assert!(matches!(
        made("analyst", "pod-1", Duration::ZERO).await,
        Err(Error::InvalidInput(_))
    ));
}

#[tokio::test]
async fn given_valid_values_and_a_bus_that_cannot_be_used_when_the_client_is_made_then_unavailable_not_invalid_input()
 {
    match made("analyst_1", "pod-1", Duration::from_millis(300)).await {
        Err(Error::Unavailable(_)) => {}
        other => panic!("{:?}", other.map(|_| ())),
    }
}

#[tokio::test]
async fn given_a_client_from_a_backend_then_it_names_its_runner_type_and_its_instance_key() {
    let (_jobs, client) = scripted();
    assert_eq!(client.runner_type(), "analyst");
    assert_eq!(client.instance_key(), "pod-1");
}
