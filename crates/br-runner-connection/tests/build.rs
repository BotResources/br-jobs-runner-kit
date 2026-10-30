mod support;

use std::sync::Arc;
use std::time::Duration;

use br_runner_connection::{Connection, Error, FixedToken};
use support::*;

async fn build(endpoint: &str) -> Result<Connection, Error> {
    Connection::new(
        endpoint,
        Arc::new(FixedToken::new(TOKEN)),
        idle_bus().await,
        account(),
        Duration::from_secs(1),
    )
}

#[tokio::test]
async fn an_endpoint_that_is_not_an_absolute_http_or_https_url_is_invalid_input() {
    for bad in [
        "",
        "gateway:8080/graphql",
        "/graphql",
        "ftp://gateway/graphql",
        "ws://gateway/graphql",
        "http://",
        "https:///graphql",
        "http://exa mple.test/",
        "http://host/p#fragment",
        "http://host/p#",
        "http://user:secret@host/graphql",
        "https://user@host/graphql",
        "https://@host/graphql",
    ] {
        assert!(
            matches!(build(bad).await, Err(Error::InvalidInput(_))),
            "{bad:?} must be refused"
        );
    }
}

#[tokio::test]
async fn the_refusal_of_an_endpoint_does_not_repeat_it() {
    for bad in ["http://user:hunter2@host/graphql", "http://host/#hunter2"] {
        let error = build(bad).await.expect_err("refused");
        let text = format!("{error} | {error:?}");
        assert!(!text.contains("hunter2"), "{text}");
    }
}

#[tokio::test]
async fn a_zero_reply_wait_is_invalid_input() {
    let got = Connection::new(
        "http://127.0.0.1:1/g",
        Arc::new(FixedToken::new(TOKEN)),
        idle_bus().await,
        account(),
        Duration::ZERO,
    );
    assert!(matches!(got, Err(Error::InvalidInput(_))), "{got:?}");
}

#[tokio::test]
async fn http_and_https_endpoints_build_with_or_without_port_and_path() {
    for good in [
        "http://gateway:8080/graphql",
        "http://gateway",
        "https://gateway.example.test/graphql?x=1",
        "HTTPS://gateway.example.test",
        "http://127.0.0.1:9/g",
        "http://[::1]:9/g",
    ] {
        assert!(build(good).await.is_ok(), "{good:?} must build");
    }
}

#[tokio::test]
async fn building_sends_nothing_and_checks_nothing_on_the_network() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let port = listener.local_addr().expect("addr").port();
    let connection = build(&format!("http://127.0.0.1:{port}/g"))
        .await
        .expect("builds");
    tokio::task::yield_now().await;
    assert!(
        matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "nothing connected"
    );
    drop(connection);
    let closed = build("http://127.0.0.1:1/g").await;
    assert!(closed.is_ok(), "a closed port does not fail the building");
}

#[tokio::test]
async fn the_connection_is_send_sync_and_clone() {
    fn shareable<T: Send + Sync + Clone + 'static>() {}
    shareable::<Connection>();
}

#[tokio::test]
async fn the_connection_gives_back_the_service_account() {
    let connection = build("http://127.0.0.1:1/g").await.expect("builds");
    assert_eq!(connection.service_account(), account());
}
