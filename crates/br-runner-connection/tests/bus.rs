mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use br_runner_connection::{Cause, Connection, Error};
use bytes::Bytes;
use futures::StreamExt;
use support::nats::Server;
use support::*;

const WAIT: Duration = Duration::from_secs(10);

async fn connect_to(client: async_nats::Client) -> (Connection, Arc<Counting>) {
    let credential = Counting::giving_token();
    let connection = Connection::new(
        "http://127.0.0.1:1/graphql",
        credential.clone(),
        client,
        account(),
        WAIT,
    )
    .expect("builds");
    (connection, credential)
}

async fn echo(client: async_nats::Client, subject: &'static str) {
    let mut subscription = client.subscribe(subject).await.expect("subscribed");
    client.flush().await.expect("flushed");
    tokio::spawn(async move {
        while let Some(message) = subscription.next().await {
            if let Some(reply) = message.reply {
                let mut body = b"echo:".to_vec();
                body.extend_from_slice(&message.payload);
                let _ = client.publish(reply, body.into()).await;
            }
        }
    });
}

async fn request(
    connection: &Connection,
    subject: &str,
    body: Bytes,
    wait: Duration,
) -> Result<Bytes, Error> {
    tokio::time::timeout(
        Duration::from_secs(15),
        connection.bus_request(subject, body, wait),
    )
    .await
    .expect("the request returned")
}

async fn connected(connection: &Connection) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while connection.bus().connection_state() != async_nats::connection::State::Connected {
        assert!(Instant::now() < deadline, "the bus did not reconnect");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn the_connection_gives_back_the_bus_it_was_built_with_and_binds_nothing_on_it() {
    let server = Server::start();
    let client = server.client().await;
    let (connection, _) = connect_to(client.clone()).await;
    assert_eq!(connection.service_account(), account());
    let mut subscription = client.subscribe("probe").await.expect("subscribed");
    client.flush().await.expect("flushed");
    connection
        .bus()
        .publish("probe", Bytes::from_static(b"through the given bus"))
        .await
        .expect("published");
    let message = tokio::time::timeout(WAIT, subscription.next())
        .await
        .expect("in time")
        .expect("a message");
    assert_eq!(
        message.payload,
        Bytes::from_static(b"through the given bus")
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_request_gives_the_body_of_its_one_reply_and_never_asks_the_credential() {
    let server = Server::start();
    let service = server.client().await;
    echo(service, "svc.echo").await;
    let (connection, credential) = connect_to(server.client().await).await;
    let reply = request(&connection, "svc.echo", Bytes::from_static(b"ping"), WAIT)
        .await
        .expect("a reply");
    assert_eq!(reply, Bytes::from_static(b"echo:ping"));
    assert_eq!(credential.fetches(), 0);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_request_that_no_service_listens_to_is_unavailable() {
    let server = Server::start();
    let (connection, _) = connect_to(server.client().await).await;
    match request(&connection, "svc.nobody", Bytes::new(), WAIT).await {
        Err(Error::Unavailable(failure)) => assert_eq!(failure.cause, Cause::Transport),
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_request_with_no_reply_within_the_wait_is_lost_by_the_wait_and_is_sent_once() {
    let server = Server::start();
    let service = server.client().await;
    let mut subscription = service.subscribe("svc.mute").await.expect("subscribed");
    service.flush().await.expect("flushed");
    let (connection, _) = connect_to(server.client().await).await;
    let started = Instant::now();
    let got = request(
        &connection,
        "svc.mute",
        Bytes::from_static(b"once"),
        Duration::from_millis(300),
    )
    .await;
    assert!(
        matches!(got, Err(Error::Lost(ref f)) if f.cause == Cause::WaitFired),
        "{got:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the wait of the call applies"
    );
    connection.bus().flush().await.expect("flushed");
    let sentinel = server.client().await;
    sentinel
        .publish("svc.mute", Bytes::from_static(b"sentinel"))
        .await
        .expect("published");
    sentinel.flush().await.expect("flushed");
    let first = tokio::time::timeout(WAIT, subscription.next())
        .await
        .expect("in time")
        .expect("m");
    let second = tokio::time::timeout(WAIT, subscription.next())
        .await
        .expect("in time")
        .expect("m");
    assert_eq!(first.payload, Bytes::from_static(b"once"));
    assert_eq!(
        second.payload,
        Bytes::from_static(b"sentinel"),
        "the request was not sent again"
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_subject_the_bus_refuses_is_invalid_input() {
    let server = Server::start();
    let (connection, _) = connect_to(server.client().await).await;
    for subject in ["", "a b"] {
        let got = request(&connection, subject, Bytes::new(), WAIT).await;
        assert!(
            matches!(got, Err(Error::InvalidInput(_))),
            "{subject:?}: {got:?}"
        );
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_request_in_flight_during_a_cut_of_the_session_is_lost_by_transport() {
    let mut server = Server::start();
    let service = server.client().await;
    let mut subscription = service.subscribe("svc.cut").await.expect("subscribed");
    service.flush().await.expect("flushed");
    let (connection, _) = connect_to(server.client().await).await;
    let call = tokio::spawn({
        let connection = connection.clone();
        async move {
            request(
                &connection,
                "svc.cut",
                Bytes::from_static(b"x"),
                Duration::from_secs(2),
            )
            .await
        }
    });
    tokio::time::timeout(WAIT, subscription.next())
        .await
        .expect("in time")
        .expect("m");
    server.kill();
    let got = call.await.expect("joined");
    assert!(
        matches!(got, Err(Error::Lost(ref f)) if f.cause == Cause::Transport),
        "{got:?}"
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_request_with_no_connected_bus_is_unavailable_and_is_not_delivered_after_the_reconnection()
 {
    let mut server = Server::start();
    let (connection, _) = connect_to(server.client().await).await;
    server.kill();
    let deadline = Instant::now() + Duration::from_secs(10);
    while connection.bus().connection_state() == async_nats::connection::State::Connected {
        assert!(Instant::now() < deadline, "the cut was not seen");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let got = request(
        &connection,
        "svc.late",
        Bytes::from_static(b"late"),
        Duration::from_millis(200),
    )
    .await;
    assert!(
        matches!(got, Err(Error::Unavailable(ref f)) if f.cause == Cause::Transport),
        "{got:?}"
    );
    server.restart();
    let service = server.client().await;
    let mut subscription = service.subscribe("svc.late").await.expect("subscribed");
    service.flush().await.expect("flushed");
    connected(&connection).await;
    connection.bus().flush().await.expect("flushed");
    service
        .publish("svc.late", Bytes::from_static(b"sentinel"))
        .await
        .expect("published");
    service.flush().await.expect("flushed");
    let first = tokio::time::timeout(WAIT, subscription.next())
        .await
        .expect("in time")
        .expect("m");
    assert_eq!(
        first.payload,
        Bytes::from_static(b"sentinel"),
        "nothing was repeated"
    );
}
