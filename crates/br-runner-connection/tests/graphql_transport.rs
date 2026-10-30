mod support;

use std::time::Duration;

use br_runner_connection::{Cause, CodeOrigin, Error};
use serde_json::{Value, json};
use support::*;

const SHORT: Duration = Duration::from_millis(400);
const LONG: Duration = Duration::from_secs(10);

async fn run(
    server: &HttpServer,
    credential: std::sync::Arc<Counting>,
    wait: Duration,
) -> Result<Value, Error> {
    let connection = connection_to(&server.url(), credential, wait).await;
    bounded(connection.graphql("{ x }", json!({}))).await
}

fn unused_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

#[tokio::test]
async fn a_connection_that_cannot_be_made_is_unavailable_by_transport() {
    let url = format!("http://127.0.0.1:{}/g", unused_port());
    let connection = connection_to(&url, Counting::giving_token(), LONG).await;
    match bounded(connection.graphql("{ x }", json!({}))).await {
        Err(Error::Unavailable(failure)) => assert_eq!(failure.cause, Cause::Transport),
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[tokio::test]
async fn a_failed_tls_handshake_is_unavailable_by_transport_and_a_silent_one_by_the_wait() {
    let closing = HttpServer::start(Mode::Drop).await;
    let url = closing.url().replace("http://", "https://");
    let connection = connection_to(&url, Counting::giving_token(), LONG).await;
    assert!(matches!(
        bounded(connection.graphql("{ x }", json!({}))).await,
        Err(Error::Unavailable(f)) if f.cause == Cause::Transport
    ));
    let silent = HttpServer::start(Mode::Mute).await;
    let url = silent.url().replace("http://", "https://");
    let connection = connection_to(&url, Counting::giving_token(), SHORT).await;
    assert!(matches!(
        bounded(connection.graphql("{ x }", json!({}))).await,
        Err(Error::Unavailable(f)) if f.cause == Cause::WaitFired
    ));
}

#[tokio::test]
async fn a_server_that_never_reads_makes_the_wait_fire_after_the_connection_was_ready_so_lost() {
    let server = HttpServer::start(Mode::Mute).await;
    let got = run(&server, Counting::giving_token(), SHORT).await;
    assert!(
        matches!(got, Err(Error::Lost(ref f)) if f.cause == Cause::WaitFired),
        "{got:?}"
    );
    assert_eq!(server.accepted(), 1);
}

#[tokio::test]
async fn a_server_that_reads_and_never_answers_is_lost_by_the_wait_and_gets_the_request_once() {
    let server = HttpServer::start(serve(|_| Action::Hang)).await;
    let got = run(&server, Counting::giving_token(), SHORT).await;
    assert!(
        matches!(got, Err(Error::Lost(ref f)) if f.cause == Cause::WaitFired),
        "{got:?}"
    );
    assert_eq!(server.requests().len(), 1);
    assert_eq!(server.accepted(), 1);
}

#[tokio::test]
async fn an_interrupted_or_cut_request_is_lost_by_transport_and_is_not_sent_again() {
    let behaviours: Vec<(&str, Behaviour)> = vec![
        ("closed after reading", Box::new(|| Action::Close)),
        (
            "closed in the middle of the body",
            Box::new(|| {
                Action::RawThenClose(
                    b"HTTP/1.1 200 X\r\ncontent-length: 100\r\n\r\n{\"data\"".to_vec(),
                )
            }),
        ),
        (
            "not HTTP",
            Box::new(|| Action::Raw(b"THIS IS NOT HTTP\r\n\r\n".to_vec())),
        ),
    ];
    for (name, behaviour) in behaviours {
        let server = HttpServer::start(serve(move |_| behaviour())).await;
        let credential = Counting::giving_token();
        let got = run(&server, credential.clone(), LONG).await;
        assert!(
            matches!(got, Err(Error::Lost(ref f)) if f.cause == Cause::Transport),
            "{name}: {got:?}"
        );
        assert_eq!(server.requests().len(), 1, "{name}");
        assert_eq!(server.accepted(), 1, "{name}");
        assert_eq!(credential.fetches(), 1, "{name}");
    }
}

#[tokio::test]
async fn a_server_that_closes_at_once_is_lost_and_nothing_is_sent_again() {
    let server = HttpServer::start(Mode::Drop).await;
    let got = run(&server, Counting::giving_token(), LONG).await;
    assert!(
        matches!(got, Err(Error::Lost(ref f)) if f.cause == Cause::Transport),
        "{got:?}"
    );
    assert_eq!(server.accepted(), 1);
}

#[tokio::test]
async fn after_a_rejection_no_other_token_is_asked_and_nothing_is_sent_again() {
    let rejections: Vec<Behaviour> = vec![
        Box::new(|| Action::Reply(401, Vec::new())),
        Box::new(|| {
            Action::json(
                200,
                json!({ "errors": [{ "extensions": { "code": "UNAUTHENTICATED" } }] }),
            )
        }),
    ];
    for rejection in rejections {
        let server = HttpServer::start(serve(move |_| rejection())).await;
        let credential = Counting::giving_token();
        let got = run(&server, credential.clone(), LONG).await;
        assert!(matches!(got, Err(Error::Unauthenticated)), "{got:?}");
        assert_eq!(credential.fetches(), 1);
        assert_eq!(server.requests().len(), 1);
        assert_eq!(server.accepted(), 1);
    }
}

#[tokio::test]
async fn every_failure_leaves_the_connection_usable_for_the_next_call() {
    let server = HttpServer::start(serve(|request| {
        if request.json()["query"] == "{ bad }" {
            Action::Close
        } else {
            data(json!({ "ok": true }))
        }
    }))
    .await;
    let connection = connection_to(&server.url(), Counting::giving_token(), LONG).await;
    assert!(
        bounded(connection.graphql("{ bad }", json!({})))
            .await
            .is_err()
    );
    assert_eq!(
        bounded(connection.graphql("{ good }", json!({})))
            .await
            .expect("data"),
        json!({ "ok": true })
    );
}

#[tokio::test]
async fn a_401_whose_body_is_cut_or_never_finishes_is_still_unauthenticated() {
    let behaviours: Vec<(&str, Behaviour)> = vec![
        (
            "cut",
            Box::new(|| {
                Action::RawThenClose(
                    b"HTTP/1.1 401 X\r\ncontent-length: 100\r\n\r\n{\"err".to_vec(),
                )
            }),
        ),
        (
            "never finished",
            Box::new(|| {
                Action::Raw(b"HTTP/1.1 401 X\r\ncontent-length: 100\r\n\r\n{\"err".to_vec())
            }),
        ),
    ];
    for (name, behaviour) in behaviours {
        let server = HttpServer::start(serve(move |_| behaviour())).await;
        let got = run(&server, Counting::giving_token(), LONG).await;
        assert!(
            matches!(got, Err(Error::Unauthenticated)),
            "{name}: {got:?}"
        );
        assert_eq!(server.requests().len(), 1, "{name}");
    }
}

#[tokio::test]
async fn a_403_whose_body_is_cut_is_still_refused_forbidden() {
    let server = HttpServer::start(serve(|_| {
        Action::RawThenClose(b"HTTP/1.1 403 X\r\ncontent-length: 100\r\n\r\n{\"err".to_vec())
    }))
    .await;
    let got = run(&server, Counting::giving_token(), LONG).await;
    match got {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.code, "FORBIDDEN");
            assert_eq!(refusal.origin, CodeOrigin::Connection);
        }
        other => panic!("expected Refused, got {other:?}"),
    }
    assert_eq!(server.requests().len(), 1);
}
