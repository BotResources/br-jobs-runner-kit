mod support;

use std::time::Duration;

use br_runner_connection::{Cause, Error};
use serde_json::{Value, json};
use support::*;

const WAIT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn an_empty_document_is_invalid_input_and_nothing_is_sent_and_no_token_is_asked() {
    let server = HttpServer::start(serve(|_| data(json!({})))).await;
    let credential = Counting::giving_token();
    let connection = connection_to(&server.url(), credential.clone(), WAIT).await;
    let got = connection.graphql("", json!({})).await;
    assert!(matches!(got, Err(Error::InvalidInput(_))), "{got:?}");
    assert_eq!(credential.fetches(), 0);
    assert_eq!(server.accepted(), 0);
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn variables_that_are_not_a_json_object_are_invalid_input_and_nothing_is_sent() {
    let server = HttpServer::start(serve(|_| data(json!({})))).await;
    let credential = Counting::giving_token();
    let connection = connection_to(&server.url(), credential.clone(), WAIT).await;
    for variables in [json!([]), json!([1]), json!("x"), json!(7), json!(true)] {
        let got = connection.graphql("{ x }", variables.clone()).await;
        assert!(
            matches!(got, Err(Error::InvalidInput(_))),
            "{variables}: {got:?}"
        );
    }
    assert_eq!(credential.fetches(), 0);
    assert_eq!(server.accepted(), 0);
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn null_variables_are_read_as_no_variables() {
    let server = HttpServer::start(serve(|_| data(json!({ "ok": true })))).await;
    let connection = connection_to(&server.url(), Counting::giving_token(), WAIT).await;
    let got = connection
        .graphql("{ x }", Value::Null)
        .await
        .expect("data");
    assert_eq!(got, json!({ "ok": true }));
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].json(),
        json!({ "query": "{ x }", "variables": {} })
    );
}

#[tokio::test]
async fn a_data_that_is_not_an_object_with_no_error_is_unreadable() {
    for body in [
        json!({ "data": [] }),
        json!({ "data": [{ "a": 1 }] }),
        json!({ "data": "x" }),
        json!({ "data": 5 }),
        json!({ "data": true }),
        json!({ "data": [], "errors": [] }),
        json!({ "data": 5, "errors": null }),
    ] {
        let sent = body.clone();
        let got = answered(move || Action::json(200, sent.clone())).await;
        assert!(matches!(got, Err(Error::Unreadable(_))), "{body}: {got:?}");
    }
}

#[tokio::test]
async fn an_empty_object_as_data_is_data() {
    let got = answered(|| data(json!({}))).await.expect("data");
    assert_eq!(got, json!({}));
}

#[tokio::test]
async fn no_redirect_is_followed_and_the_answer_is_lost() {
    let target = HttpServer::start(serve(|_| data(json!({ "followed": true })))).await;
    for status in [
        "301 Moved Permanently",
        "302 Found",
        "307 Temporary Redirect",
        "308 Permanent Redirect",
    ] {
        let location = target.url();
        let server = HttpServer::start(serve(move |_| {
            Action::Raw(
                format!("HTTP/1.1 {status}\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n")
                    .into_bytes(),
            )
        }))
        .await;
        let connection = connection_to(&server.url(), Counting::giving_token(), WAIT).await;
        match connection.graphql("{ x }", json!({})).await {
            Err(Error::Lost(failure)) => assert_eq!(failure.cause, Cause::Transport, "{status}"),
            other => panic!("{status}: expected Lost, got {other:?}"),
        }
        assert_eq!(server.requests().len(), 1, "{status}");
    }
    assert_eq!(target.accepted(), 0);
    assert!(target.requests().is_empty());
}
