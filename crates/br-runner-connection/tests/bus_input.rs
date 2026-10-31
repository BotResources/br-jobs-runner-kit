mod support;

use std::time::Duration;

use br_runner_connection::{Cause, Error};
use bytes::Bytes;
use futures::StreamExt;
use support::nats::{Server, WAIT, connect_to, echo, request};

async fn next_payload(subscription: &mut async_nats::Subscriber) -> Bytes {
    tokio::time::timeout(WAIT, subscription.next())
        .await
        .expect("in time")
        .expect("a message")
        .payload
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_subject_with_a_wildcard_a_whitespace_or_an_empty_token_is_invalid_input_and_nothing_is_sent()
 {
    let server = Server::start();
    let (connection, _) = connect_to(server.client().await).await;
    let service = server.client().await;
    let mut everything = service.subscribe(">").await.expect("subscribed");
    service.flush().await.expect("flushed");
    let refused = [
        "",
        " ",
        "a b",
        "a\tb",
        "a\u{a0}b",
        "a\u{2003}b",
        "a\r\nb",
        "*",
        ">",
        "svc.*",
        "svc.>",
        "*.echo",
        "svc.*.echo",
        ".svc",
        "svc.",
        "svc..echo",
    ];
    for subject in refused {
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
    assert_eq!(
        next_payload(&mut everything).await,
        Bytes::from_static(b"sentinel"),
        "nothing was sent for a refused subject"
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_literal_subject_with_a_star_or_a_greater_than_inside_a_token_is_sent() {
    let server = Server::start();
    let service = server.client().await;
    echo(service, "svc.a*b").await;
    let (connection, _) = connect_to(server.client().await).await;
    let reply = request(&connection, "svc.a*b", Bytes::from_static(b"x"), WAIT)
        .await
        .expect("a reply");
    assert_eq!(reply, Bytes::from_static(b"echo:x"));
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_body_the_bus_refuses_by_its_size_is_invalid_input_and_nothing_is_sent() {
    let server = Server::start_with_max_payload(1024);
    let service = server.client().await;
    echo(service.clone(), "svc.echo").await;
    let mut everything = service.subscribe("svc.>").await.expect("subscribed");
    service.flush().await.expect("flushed");
    let (connection, _) = connect_to(server.client().await).await;
    let too_large = Bytes::from(vec![7u8; 4096]);
    let got = request(&connection, "svc.echo", too_large, WAIT).await;
    assert!(matches!(got, Err(Error::InvalidInput(_))), "{got:?}");
    let at_the_limit = Bytes::from(vec![7u8; 1019]);
    let reply = request(&connection, "svc.echo", at_the_limit, WAIT)
        .await
        .expect("a body at the limit goes through and the session is not cut");
    assert_eq!(reply.len(), 1024);
    let first = next_payload(&mut everything).await;
    assert_eq!(first.len(), 1019, "the oversized body never left");
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn the_three_causes_tell_a_wait_a_transport_failure_and_a_missing_listener_apart() {
    let server = Server::start();
    let service = server.client().await;
    let _mute = service.subscribe("svc.mute").await.expect("subscribed");
    service.flush().await.expect("flushed");
    let (connection, _) = connect_to(server.client().await).await;
    let nobody = request(&connection, "svc.nobody", Bytes::new(), WAIT).await;
    let silent = request(
        &connection,
        "svc.mute",
        Bytes::new(),
        Duration::from_millis(200),
    )
    .await;
    assert!(
        matches!(nobody, Err(Error::Unavailable(ref f)) if f.cause == Cause::NoListener),
        "{nobody:?}"
    );
    assert!(
        matches!(silent, Err(Error::Lost(ref f)) if f.cause == Cause::WaitFired),
        "{silent:?}"
    );
}
