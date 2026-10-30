mod support;

use std::time::Duration;

use br_runner_connection::Error;
use serde_json::json;
use support::*;

const WAIT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn a_success_gives_the_data_of_the_response() {
    let server = HttpServer::start(serve(|_| data(json!({ "me": { "id": "u1" } })))).await;
    let connection = connection_to(&server.url(), Counting::giving_token(), WAIT).await;
    let data = connection
        .graphql("query { me { id } }", json!({ "a": 1 }))
        .await
        .expect("data");
    assert_eq!(data, json!({ "me": { "id": "u1" } }));
}

#[tokio::test]
async fn the_request_holds_the_document_the_variables_and_the_bearer_token_and_nothing_else() {
    let server = HttpServer::start(serve(|_| data(json!({})))).await;
    let connection = connection_to(&server.url(), Counting::giving_token(), WAIT).await;
    connection
        .graphql("query Q { x }", json!({ "k": [1, 2] }))
        .await
        .expect("data");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.target, "/graphql");
    assert_eq!(
        request.json(),
        json!({ "query": "query Q { x }", "variables": { "k": [1, 2] } })
    );
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert_eq!(
        request.header_names(),
        ["authorization", "content-length", "content-type", "host"],
        "no header the importer did not ask for, no Passport header"
    );
}

#[tokio::test]
async fn the_credential_is_asked_before_each_request_and_only_then() {
    let server = HttpServer::start(serve(|_| data(json!({})))).await;
    let credential = Counting::giving_token();
    let connection = connection_to(&server.url(), credential.clone(), WAIT).await;
    assert_eq!(credential.fetches(), 0, "building asks nothing");
    for expected in 1..=3 {
        connection.graphql("{ x }", json!({})).await.expect("data");
        assert_eq!(credential.fetches(), expected);
    }
}

#[tokio::test]
async fn the_time_the_credential_takes_does_not_count_in_the_reply_wait() {
    let server = HttpServer::start(serve(|_| data(json!({ "ok": true })))).await;
    let wait = Duration::from_millis(300);
    let (credential, release) = Counting::gated();
    let connection = connection_to(&server.url(), credential.clone(), wait).await;
    let call = tokio::spawn({
        let connection = connection.clone();
        async move { connection.graphql("{ x }", json!({})).await }
    });
    eventually("the credential to be asked", || credential.fetches() == 1).await;
    tokio::time::sleep(wait * 3).await;
    release.send(()).expect("released");
    let data = call
        .await
        .expect("joined")
        .expect("the wait did not count the token");
    assert_eq!(data, json!({ "ok": true }));
}

#[tokio::test]
async fn several_tasks_share_one_connection() {
    let server = HttpServer::start(serve(|_| data(json!({ "ok": true })))).await;
    let credential = Counting::giving_token();
    let connection = connection_to(&server.url(), credential.clone(), WAIT).await;
    let calls: Vec<_> = (0..8)
        .map(|_| {
            let connection = connection.clone();
            tokio::spawn(async move { connection.graphql("{ x }", json!({})).await })
        })
        .collect();
    for call in calls {
        call.await.expect("joined").expect("data");
    }
    assert_eq!(server.requests().len(), 8);
    assert_eq!(credential.fetches(), 8);
}

#[tokio::test]
async fn an_abandoned_call_leaves_nothing_behind() {
    let server = HttpServer::start(serve(|_| Action::Hang)).await;
    let connection = connection_to(&server.url(), Counting::giving_token(), WAIT).await;
    let call = tokio::spawn({
        let connection = connection.clone();
        async move { connection.graphql("{ x }", json!({})).await }
    });
    eventually("the request to arrive", || server.requests().len() == 1).await;
    call.abort();
    eventually("the socket to close", || server.closed() == 1).await;
}

#[tokio::test]
async fn a_credential_that_fails_sends_nothing() {
    use br_runner_connection::{CredentialFailure, CredentialKind};
    let server = HttpServer::start(serve(|_| data(json!({})))).await;
    for (failure, kind) in [
        (
            CredentialFailure::no_usable("nothing to give"),
            CredentialKind::NoUsable,
        ),
        (
            CredentialFailure::fetch_failed("identity said no"),
            CredentialKind::FetchFailed,
        ),
    ] {
        let text = failure.text.clone();
        let connection = connection_to(&server.url(), Counting::failing(failure), WAIT).await;
        match connection.graphql("{ x }", json!({})).await {
            Err(Error::Credential(got)) => {
                assert_eq!(got.kind, kind);
                assert_eq!(got.text, text);
            }
            other => panic!("expected a credential failure, got {other:?}"),
        }
    }
    assert_eq!(server.accepted(), 0, "nothing was sent");
}

#[tokio::test]
async fn a_token_that_is_not_a_header_value_is_no_usable_credential_and_sends_nothing() {
    use br_runner_connection::{CredentialKind, FixedToken};
    let server = HttpServer::start(serve(|_| data(json!({})))).await;
    for bad in ["", "two\nlines"] {
        let connection = connection_to(
            &server.url(),
            std::sync::Arc::new(FixedToken::new(bad)),
            WAIT,
        )
        .await;
        match connection.graphql("{ x }", json!({})).await {
            Err(Error::Credential(got)) => assert_eq!(got.kind, CredentialKind::NoUsable),
            other => panic!("expected a credential failure, got {other:?}"),
        }
    }
    assert_eq!(server.accepted(), 0);
}
