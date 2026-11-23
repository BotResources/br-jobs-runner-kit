use br_core_kernel::ServiceAccountId;
use br_runner_connection::{
    BoxFuture, Connection, Credential, CredentialFailure, FixedToken, Token,
};
use contract_jobs::graphql_roots::PAYLOAD_ROOT;

use super::http_server::{Action, HttpServer, serve};
use super::text_forms::{TOKEN, assert_clean};
use super::*;
use crate::error::{CredentialKind, Error, Operation};

struct Refusing(CredentialFailure);

impl Credential for Refusing {
    fn token(&self) -> BoxFuture<'_, Result<Token, CredentialFailure>> {
        let failure = self.0.clone();
        Box::pin(async move { Err(failure) })
    }
}

struct Giving(&'static str);

impl Credential for Giving {
    fn token(&self) -> BoxFuture<'_, Result<Token, CredentialFailure>> {
        let value = self.0;
        Box::pin(async move { Ok(Token::new(value)) })
    }
}

async fn bus() -> async_nats::Client {
    async_nats::ConnectOptions::new()
        .retry_on_initial_connect()
        .connect("nats://127.0.0.1:1")
        .await
        .expect("a client that retries")
}

async fn work_over(endpoint: &str, credential: Arc<dyn Credential>) -> (Arc<ScriptedJobs>, Work) {
    let jobs = jobs();
    let connection = Connection::new(
        endpoint,
        credential,
        bus().await,
        ServiceAccountId(id(90)),
        Duration::from_secs(2),
    )
    .expect("a connection");
    jobs.route_graphql_through(connection);
    let work = work(&jobs);
    (jobs, work)
}

async fn echo(status: u16) -> HttpServer {
    let handler = serve(move |request| {
        let echoed = request.header("authorization").unwrap_or("none").to_owned();
        Action::Reply(status, format!("rejected for {echoed}").into_bytes())
    });
    HttpServer::start(handler).await
}

fn payload_reply(success: bool) -> serde_json::Value {
    if success {
        json!({ "data": { PAYLOAD_ROOT: { "success": true, "reason": null, "message": null,
            "content": { "message": "m", "attachments": [] } } } })
    } else {
        json!({ "data": { PAYLOAD_ROOT: { "success": false, "reason": "gone",
            "message": "the text of jobs" } } })
    }
}

#[test]
#[should_panic(expected = "shows the")]
fn given_a_text_that_holds_a_job_id_the_search_finds_it() {
    assert_clean("control", &format!("job {}", id(JOB)));
}

#[test]
#[should_panic(expected = "shows the")]
fn given_a_text_that_holds_the_token_the_search_finds_it() {
    assert_clean("control", &format!("Bearer {TOKEN}"));
}

#[tokio::test]
async fn given_a_known_token_when_the_work_calls_graphql_then_the_call_carries_the_token_and_the_ids_of_the_work_and_never_the_passport()
 {
    let server = HttpServer::start(serve(|_| Action::json(200, payload_reply(true)))).await;
    let (_jobs, work) = work_over(&server.url(), Arc::new(FixedToken::new(TOKEN))).await;
    work.payload().await.expect("the payload");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    let body = String::from_utf8_lossy(&requests[0].body).into_owned();
    assert!(body.contains(&id(JOB).to_string()) && body.contains(&id(RUN).to_string()));
    for (what, needle) in super::text_forms::forbidden() {
        if what.starts_with("job id") || what == "token" {
            continue;
        }
        assert!(
            !body.contains(&needle),
            "the request shows the {what}: {body}"
        );
    }
    assert!(
        !requests[0]
            .header_names()
            .iter()
            .any(|name| name.contains("passport"))
    );
}

#[tokio::test]
async fn given_a_service_that_echoes_the_token_when_every_kind_of_failure_comes_then_no_error_text_shows_the_token_the_passport_or_the_job_ids()
 {
    let unauthenticated = echo(401).await;
    let forbidden = echo(403).await;
    let broken = echo(500).await;
    let garbage = HttpServer::start(serve(|request| {
        let echoed = request.header("authorization").unwrap_or("none").to_owned();
        Action::json(200, json!({ "data": { "echo": echoed } }))
    }))
    .await;
    let refused = HttpServer::start(serve(|_| Action::json(200, payload_reply(false)))).await;
    let mut produced = Vec::new();
    for server in [&unauthenticated, &forbidden, &broken, &garbage, &refused] {
        let (_jobs, work) = work_over(&server.url(), Arc::new(FixedToken::new(TOKEN))).await;
        produced.push(work.payload().await.expect_err("an error"));
        produced.push(work.saved_state().await.expect_err("an error"));
        produced.push(
            work.read_result(crate::model::ResultRef(id(70)), false)
                .await
                .expect_err("an error"),
        );
    }
    assert!(matches!(produced[0], Error::Unauthenticated));
    assert!(matches!(produced[3], Error::Refused(_)));
    assert!(matches!(produced[6], Error::Lost(_)));
    assert!(matches!(produced[9], Error::Unreadable(_)));
    match &produced[12] {
        Error::Refused(refusal) => {
            assert_eq!(refusal.operation, Operation::Payload);
            assert_eq!(refusal.message, "the text of jobs");
        }
        other => panic!("{other:?}"),
    }
    for error in &produced {
        assert_clean("a graphql error debug", &format!("{error:?}"));
        assert_clean("a graphql error display", &error.to_string());
    }
}

#[tokio::test]
async fn given_a_credential_that_gives_no_token_when_the_work_calls_graphql_then_the_credential_error_carries_the_text_the_credential_gave_and_nothing_is_sent()
 {
    let server = HttpServer::start(serve(|_| Action::json(200, payload_reply(true)))).await;
    for (failure, kind) in [
        (
            CredentialFailure::fetch_failed("the secret store did not answer"),
            CredentialKind::FetchFailed,
        ),
        (
            CredentialFailure::no_usable("the token has expired"),
            CredentialKind::NoUsable,
        ),
    ] {
        let text = failure.text.clone();
        let (_jobs, work) = work_over(&server.url(), Arc::new(Refusing(failure))).await;
        match work.payload().await {
            Err(Error::Credential(found)) => {
                assert_eq!(found.kind, kind);
                assert_eq!(found.text, text);
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn given_a_token_that_cannot_be_sent_or_is_empty_when_the_work_calls_graphql_then_no_usable_credential_and_the_token_is_not_echoed()
 {
    let server = HttpServer::start(serve(|_| Action::json(200, payload_reply(true)))).await;
    for token in ["", "tok-known-7f3a9c41-do-not-print\nsecond-line"] {
        let (_jobs, work) = work_over(&server.url(), Arc::new(Giving(token))).await;
        match work.payload().await {
            Err(error @ Error::Credential(_)) => {
                let Error::Credential(found) = &error else {
                    unreachable!()
                };
                assert_eq!(found.kind, CredentialKind::NoUsable);
                assert_clean("a credential error", &format!("{error:?} {error}"));
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(server.requests().is_empty());
}
