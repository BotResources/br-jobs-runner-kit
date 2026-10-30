mod support;

use std::sync::Arc;
use std::time::Duration;

use br_runner_connection::{Connection, Error, FixedToken};
use serde_json::json;
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

fn with_user_information(user: &str, password: &str) -> String {
    format!("http://{user}:{password}@host/graphql")
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
        &with_user_information("user", "secret"),
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
    for bad in [
        with_user_information("user", "hunter2"),
        "http://host/#hunter2".to_owned(),
    ] {
        let error = build(&bad).await.expect_err("refused");
        let text = format!("{error} | {error:?}");
        assert!(!text.contains("hunter2"), "{text}");
    }
}

#[tokio::test]
async fn a_port_that_is_not_a_number_from_1_to_65535_is_invalid_input_and_never_the_default() {
    for bad in [
        "http://h:65536/",
        "https://h:99999999999/",
        "http://h:-1/",
        "http://h:+80/",
        "http://h:abc/",
        "http://h:/",
        "http://h:0/",
        "https://h:0/g",
        "http://h:80x/",
        "http://[::1]:0/",
        "http://[::1]:65536/",
        "http://[::1]:/",
        "http://[::1]:abc/",
    ] {
        assert!(
            matches!(build(bad).await, Err(Error::InvalidInput(_))),
            "{bad:?} must be refused"
        );
    }
}

#[tokio::test]
async fn a_bracketed_host_that_is_not_an_ipv6_address_or_has_text_after_it_is_invalid_input() {
    for bad in [
        "http://[::1]x/",
        "http://[::1]x:80/",
        "http://[::1]80/",
        "http://[v1.fe]/",
        "http://[v1.fe]:80/",
        "http://[1.2.3.4]/",
        "http://[example.test]/",
        "http://[]/",
        "http://[::1/",
        "http://[::1%25eth0]/",
    ] {
        assert!(
            matches!(build(bad).await, Err(Error::InvalidInput(_))),
            "{bad:?} must be refused"
        );
    }
}

#[tokio::test]
async fn the_refusal_of_a_port_or_a_bracket_does_not_repeat_the_endpoint() {
    for bad in [
        "http://marker-host.test:99999/marker-path",
        "http://[marker-host]/marker-path",
        "http://[::1]marker-tail/marker-path",
    ] {
        let error = build(bad).await.expect_err("refused");
        let text = format!("{error} | {error:?}");
        assert!(!text.contains("marker"), "{text}");
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
        "http://[::1]/g",
        "http://gateway:65535/g",
        "http://gateway:1/g",
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

#[tokio::test]
async fn a_query_without_a_path_is_sent_with_the_root_path_and_the_query() {
    let server = HttpServer::start(serve(|_| data(json!({})))).await;
    for (suffix, target) in [("?x=1", "/?x=1"), ("/?x=1", "/?x=1"), ("", "/")] {
        let endpoint = format!("http://127.0.0.1:{}{suffix}", server.port());
        let connection = build(&endpoint).await.expect("builds");
        connection
            .graphql("{ x }", json!({}))
            .await
            .expect("the call reaches the server");
        let requests = server.requests();
        assert_eq!(requests.last().expect("a request").target, target);
    }
    assert_eq!(server.requests().len(), 3);
}
