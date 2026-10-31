use std::time::Duration;

use br_util_nats_fabric::{INTEGRATION_CMD, INTEGRATION_EVT, KV_PUBLISHED_LANGUAGE};
use contract_jobs::runner::{
    CANCEL_BUCKET, LOG_STREAM, PRESENCE_BUCKET, STATUS_STREAM, TRIGGER_STREAM,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::live_world::{WAIT, World, connection_on};
use super::{live, refused_connect};
use crate::error::{Cause, Error};

fn infra_missing_naming(error: &Error, object: &str) {
    match error {
        Error::InfraMissing(text) => {
            assert!(text.contains(object), "{text} does not name {object}")
        }
        other => panic!("expected InfraMissing for {object}, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_every_object_when_connecting_then_the_transport_is_made_and_creates_no_infra() {
    let world = World::ready().await;
    let _live = live(&world, "analyst", "pod-1").await;
    let stream = world.js.get_stream(TRIGGER_STREAM).await.expect("stream");
    assert_eq!(stream.cached_info().state.consumer_count, 0);
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_missing_stream_when_connecting_then_infra_missing_names_it() {
    for missing in [TRIGGER_STREAM, STATUS_STREAM, LOG_STREAM] {
        let world = World::ready().await;
        world.delete_stream(missing).await;
        let error = refused_connect(&world, "analyst", "pod-1", WAIT).await;
        infra_missing_naming(&error, missing);
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_missing_bucket_when_connecting_then_infra_missing_names_it() {
    for missing in [CANCEL_BUCKET, PRESENCE_BUCKET, KV_PUBLISHED_LANGUAGE] {
        let world = World::ready().await;
        world.delete_bucket(missing).await;
        let error = refused_connect(&world, "analyst", "pod-1", WAIT).await;
        infra_missing_naming(&error, missing);
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_missing_integration_stream_when_connecting_then_infra_missing_names_it() {
    for missing in [INTEGRATION_EVT, INTEGRATION_CMD] {
        let world = World::ready().await;
        world.delete_stream(missing).await;
        let error = refused_connect(&world, "analyst", "pod-1", WAIT).await;
        infra_missing_naming(&error, missing);
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_presence_bucket_without_time_to_live_when_connecting_then_infra_missing() {
    let world = World::bare().await;
    world.create_runner_streams().await;
    world.create_runner_buckets(None).await;
    world.create_platform().await;
    let error = refused_connect(&world, "analyst", "pod-1", WAIT).await;
    infra_missing_naming(&error, PRESENCE_BUCKET);
    assert!(error.to_string().contains("time to live"));
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_bad_segment_when_connecting_then_invalid_input() {
    let world = World::ready().await;
    for (runner_type, instance_key) in [
        ("", "pod-1"),
        ("ana.lyst", "pod-1"),
        ("ana lyst", "pod-1"),
        ("*", "pod-1"),
        (">", "pod-1"),
        ("analyst", ""),
        ("analyst", "pod.1"),
        ("analyst", "pod 1"),
        ("analyst", "pod-*"),
        ("analyst", ">"),
    ] {
        let error = refused_connect(&world, runner_type, instance_key, WAIT).await;
        assert!(
            matches!(error, Error::InvalidInput(_)),
            "{runner_type:?} {instance_key:?} gave {error:?}"
        );
    }
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_zero_reply_wait_when_connecting_then_invalid_input() {
    let world = World::ready().await;
    let error = refused_connect(&world, "analyst", "pod-1", Duration::ZERO).await;
    assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_bus_that_is_not_connected_when_connecting_then_unavailable() {
    let client = async_nats::ConnectOptions::new()
        .retry_on_initial_connect()
        .connect("nats://127.0.0.1:1")
        .await
        .expect("a client that retries");
    let error =
        match super::Live::connect("analyst", "pod-1", WAIT, connection_on(&client, WAIT)).await {
            Err(error) => error,
            Ok(_) => panic!("connected"),
        };
    assert!(matches!(error, Error::Unavailable(_)), "{error:?}");
}

async fn silent_bus() -> (u16, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bound");
    let port = listener.local_addr().expect("address").port();
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let info = "INFO {\"server_id\":\"stand-in\",\"version\":\"2.10.0\",\"proto\":1,\"headers\":false,\"max_payload\":1048576}\r\n";
                let _ = socket.write_all(info.as_bytes()).await;
                let mut buffer = [0u8; 4096];
                while let Ok(read) = socket.read(&mut buffer).await {
                    if read == 0 {
                        break;
                    }
                    if String::from_utf8_lossy(&buffer[..read]).contains("PING") {
                        let _ = socket.write_all(b"PONG\r\n").await;
                    }
                }
            });
        }
    });
    (port, task)
}

#[tokio::test]
#[ignore = "needs nats-server -js on the path"]
async fn given_a_bus_that_does_not_answer_a_check_when_connecting_then_unavailable_with_a_wait_fired()
 {
    let (port, _task) = silent_bus().await;
    let client = async_nats::connect(format!("127.0.0.1:{port}"))
        .await
        .expect("connects");
    let wait = Duration::from_millis(300);
    let started = std::time::Instant::now();
    let error =
        match super::Live::connect("analyst", "pod-1", wait, connection_on(&client, wait)).await {
            Err(error) => error,
            Ok(_) => panic!("connected"),
        };
    match error {
        Error::Unavailable(failure) => assert_eq!(failure.cause, Cause::WaitFired),
        other => panic!("expected Unavailable, got {other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(5));
}
