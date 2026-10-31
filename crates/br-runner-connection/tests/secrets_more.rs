mod support;

use std::time::Duration;

use br_runner_connection::Error;
use bytes::Bytes;
use serde_json::json;
use support::nats::{Server, connect_to};
use support::*;

fn rendered(error: &Error) -> String {
    format!("{error} | {error:?}")
}

async fn graphql_failing(action: impl Fn() -> Action + Send + Sync + 'static) -> Error {
    answered(action).await.expect_err("the call fails")
}

#[tokio::test]
async fn no_text_of_an_input_a_redirect_or_a_malformed_answer_failure_holds_the_token() {
    let server = HttpServer::start(serve(|_| data(json!({})))).await;
    let connection = connection_to(
        &server.url(),
        Counting::giving_token(),
        Duration::from_secs(10),
    )
    .await;
    let mut errors = vec![
        connection.graphql("", json!({})).await.expect_err("empty"),
        connection
            .graphql("{ x }", json!([TOKEN]))
            .await
            .expect_err("not an object"),
        connection
            .graphql("{ x }", json!(TOKEN))
            .await
            .expect_err("not an object"),
    ];
    for body in [
        json!({ "data": [TOKEN] }),
        json!({ "data": TOKEN }),
        json!({ "errors": [{ "message": TOKEN, "extensions": { "code": "UNAUTHENTICATED", "reason": 5 } }] }),
        json!({ "errors": [{ "message": TOKEN, "extensions": { "reason": TOKEN } }] }),
    ] {
        for status in [200, 500] {
            let body = body.clone();
            errors.push(graphql_failing(move || Action::json(status, body.clone())).await);
        }
    }
    let target = HttpServer::start(serve(|_| data(json!({})))).await;
    let location = format!("{}?token={TOKEN}", target.url());
    errors.push(
        graphql_failing(move || {
            Action::Raw(
                format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n")
                    .into_bytes(),
            )
        })
        .await,
    );
    assert!(target.requests().is_empty());
    assert!(errors.len() >= 12);
    for error in &errors {
        let text = rendered(error);
        assert!(!text.contains(TOKEN), "a token leaked in {text}");
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn no_text_of_a_refused_bus_request_holds_the_token() {
    let server = Server::start_with_max_payload(1024);
    let (connection, _) = connect_to(server.client().await).await;
    let mut errors = Vec::new();
    for (subject, body) in [
        ("svc.*", Bytes::from_static(b"x")),
        ("svc.>", Bytes::from_static(b"x")),
        ("svc..echo", Bytes::from_static(b"x")),
        (".svc", Bytes::from_static(b"x")),
        ("svc.a\u{2003}b", Bytes::from_static(b"x")),
        ("svc.echo", Bytes::from(vec![7u8; 4096])),
    ] {
        errors.push(
            connection
                .bus_request(subject, body, Duration::from_secs(5))
                .await
                .expect_err("the request is refused"),
        );
    }
    assert!(errors.iter().all(|e| matches!(e, Error::InvalidInput(_))));
    for error in &errors {
        let text = rendered(error);
        assert!(!text.contains(TOKEN), "a token leaked in {text}");
    }
}
