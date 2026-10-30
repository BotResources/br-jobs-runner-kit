mod support;

use std::time::Duration;

use br_runner_connection::Error;
use bytes::Bytes;
use futures::{StreamExt, TryStreamExt};
use support::nats::{Server, WAIT, connect_to, echo, request};

#[tokio::test]
#[ignore = "needs a nats-server binary with JetStream"]
async fn a_connection_and_its_requests_create_no_stream_and_no_bucket() {
    let server = Server::start_with_jetstream();
    let service = server.client().await;
    echo(service, "svc.echo").await;
    let client = server.client().await;
    let jetstream = async_nats::jetstream::new(client.clone());
    let before = jetstream
        .query_account()
        .await
        .expect("JetStream is enabled");
    assert_eq!((before.streams, before.consumers), (0, 0));
    let (connection, _) = connect_to(client).await;
    request(&connection, "svc.echo", Bytes::from_static(b"ping"), WAIT)
        .await
        .expect("a reply");
    connection.bus().flush().await.expect("flushed");
    let after = jetstream.query_account().await.expect("account");
    assert_eq!((after.streams, after.consumers), (0, 0));
    let names: Vec<String> = jetstream
        .stream_names()
        .try_collect()
        .await
        .expect("stream names");
    assert!(names.is_empty(), "{names:?}");
    jetstream
        .create_key_value(async_nats::jetstream::kv::Config {
            bucket: "CONTROL".to_owned(),
            ..Default::default()
        })
        .await
        .expect("a bucket");
    assert_eq!(jetstream.query_account().await.expect("account").streams, 1);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_wait_too_long_to_count_is_no_limit_and_the_request_completes() {
    let server = Server::start();
    let service = server.client().await;
    echo(service, "svc.echo").await;
    let (connection, _) = connect_to(server.client().await).await;
    let reply = request(
        &connection,
        "svc.echo",
        Bytes::from_static(b"ping"),
        Duration::MAX,
    )
    .await
    .expect("a reply");
    assert_eq!(reply, Bytes::from_static(b"echo:ping"));
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_zero_wait_is_invalid_input_and_nothing_is_sent() {
    let server = Server::start();
    let service = server.client().await;
    let mut subscription = service.subscribe("svc.zero").await.expect("subscribed");
    service.flush().await.expect("flushed");
    let (connection, _) = connect_to(server.client().await).await;
    let got = request(
        &connection,
        "svc.zero",
        Bytes::from_static(b"never"),
        Duration::ZERO,
    )
    .await;
    assert!(matches!(got, Err(Error::InvalidInput(_))), "{got:?}");
    connection.bus().flush().await.expect("flushed");
    service
        .publish("svc.zero", Bytes::from_static(b"sentinel"))
        .await
        .expect("published");
    service.flush().await.expect("flushed");
    let first = tokio::time::timeout(WAIT, subscription.next())
        .await
        .expect("in time")
        .expect("m");
    assert_eq!(first.payload, Bytes::from_static(b"sentinel"));
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_wait_too_long_to_count_never_fires() {
    let server = Server::start();
    let service = server.client().await;
    let mut subscription = service.subscribe("svc.mute").await.expect("subscribed");
    service.flush().await.expect("flushed");
    let (connection, _) = connect_to(server.client().await).await;
    let call = connection.bus_request("svc.mute", Bytes::from_static(b"x"), Duration::MAX);
    let outcome = tokio::time::timeout(Duration::from_millis(300), call).await;
    assert!(outcome.is_err(), "the request ended: {outcome:?}");
    let received = tokio::time::timeout(WAIT, subscription.next())
        .await
        .expect("in time")
        .expect("m");
    assert_eq!(received.payload, Bytes::from_static(b"x"));
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_subject_the_bus_refuses_is_invalid_input() {
    let server = Server::start();
    let (connection, _) = connect_to(server.client().await).await;
    let service = server.client().await;
    let mut everything = service.subscribe(">").await.expect("subscribed");
    service.flush().await.expect("flushed");
    for subject in ["", "a b"] {
        let got = request(&connection, subject, Bytes::from_static(b"never"), WAIT).await;
        assert!(
            matches!(got, Err(Error::InvalidInput(_))),
            "{subject:?}: {got:?}"
        );
    }
    connection.bus().flush().await.expect("flushed");
    service
        .publish("svc.sentinel", Bytes::from_static(b"sentinel"))
        .await
        .expect("published");
    service.flush().await.expect("flushed");
    let first = tokio::time::timeout(WAIT, everything.next())
        .await
        .expect("in time")
        .expect("m");
    assert_eq!(
        first.payload,
        Bytes::from_static(b"sentinel"),
        "nothing was sent for a refused subject"
    );
}
