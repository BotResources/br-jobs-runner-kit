mod support;

use std::time::Duration;

use std::sync::Arc;

use br_runner_connection::{
    BoxFuture, Connection, Credential, CredentialFailure, Error, FixedToken, Token,
};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::{Value, json};
use support::*;

fn leaking(status: u16, body: Value) -> Action {
    Action::json(status, body)
}

fn rendered(error: &Error) -> String {
    format!("{error} | {error:?}")
}

async fn failing_with(
    action: impl Fn() -> Action + Send + Sync + 'static,
    wait: Duration,
) -> Error {
    let server = HttpServer::start(serve(move |_| action())).await;
    connection_to(&server.url(), Counting::giving_token(), wait)
        .await
        .graphql("{ x }", json!({}))
        .await
        .expect_err("the call fails")
}

#[tokio::test]
async fn no_text_of_a_failed_call_holds_the_token_whatever_the_failure() {
    let long = Duration::from_secs(10);
    let echo = format!("echo of {TOKEN} in the text");
    let mut errors = Vec::new();
    let e = echo.clone();
    errors.push(
        failing_with(
            move || {
                leaking(
                    200,
                    json!({ "data": null, "errors": [{
                        "message": e.clone(),
                        "extensions": { "code": format!("CODE_{TOKEN}"), "params": { "k": e.clone(), "list": [e.clone()] }, "reason": e.clone(), "other": e.clone() }
                    }] }),
                )
            },
            long,
        )
        .await,
    );
    let e = echo.clone();
    errors.push(
        failing_with(
            move || {
                leaking(
                    403,
                    json!({ "errors": [{ "message": e.clone(), "extensions": { "code": "NO", "params": { "k": e.clone() } } }] }),
                )
            },
            long,
        )
        .await,
    );
    let e = echo.clone();
    errors.push(failing_with(move || Action::Reply(403, e.clone().into_bytes()), long).await);
    let e = echo.clone();
    errors.push(failing_with(move || Action::Reply(502, e.clone().into_bytes()), long).await);
    let e = echo.clone();
    errors.push(failing_with(move || Action::Reply(200, e.clone().into_bytes()), long).await);
    let e = echo.clone();
    errors.push(
        failing_with(
            move || leaking(200, json!({ "errors": [{ "message": e.clone() }] })),
            long,
        )
        .await,
    );
    let e = echo.clone();
    errors.push(failing_with(move || Action::Reply(401, e.clone().into_bytes()), long).await);
    errors.push(failing_with(|| Action::Close, long).await);
    errors.push(failing_with(|| Action::Hang, Duration::from_millis(300)).await);
    let refused = HttpServer::start(Mode::Drop).await;
    let url = refused.url();
    drop(refused);
    errors.push(
        connection_to(&url, Counting::giving_token(), long)
            .await
            .graphql("{ x }", json!({}))
            .await
            .expect_err("connect refused"),
    );
    assert!(errors.len() >= 10);
    for error in &errors {
        let text = rendered(error);
        assert!(!text.contains(TOKEN), "a token leaked in {text}");
    }
    let Error::Refused(refusal) = &errors[0] else {
        panic!("expected Refused first, got {:?}", errors[0]);
    };
    assert!(refusal.message.contains("[redacted]"));
    assert!(refusal.code.contains("[redacted]"));
    assert_eq!(
        refusal.reason.as_deref(),
        Some("echo of [redacted] in the text")
    );
    assert_eq!(
        Value::Object(refusal.params.clone()),
        json!({ "k": "echo of [redacted] in the text", "list": ["echo of [redacted] in the text"] })
    );
}

struct Raw(String);

impl Credential for Raw {
    fn token(&self) -> BoxFuture<'_, Result<Token, CredentialFailure>> {
        Box::pin(async move { Ok(Token::new(self.0.clone())) })
    }
}

async fn failing_through(url: &str, credential: Arc<dyn Credential>) -> Error {
    connection_to(url, credential, Duration::from_secs(10))
        .await
        .graphql("{ x }", json!({}))
        .await
        .expect_err("the call fails")
}

#[tokio::test]
async fn no_text_of_a_tls_or_credential_failure_holds_the_token() {
    let tls = UntrustedTlsServer::start().await;
    let plain = HttpServer::start(serve(|_| data(json!({})))).await;
    let closing = HttpServer::start(Mode::Drop).await;
    let https_to_closed = closing.url().replacen("http://", "https://", 1);
    let http_to_tls = tls.url().replacen("https://", "http://", 1);
    let giving = || -> Arc<dyn Credential> { Counting::giving_token() };
    let errors = [
        failing_through(&tls.url(), giving()).await,
        failing_through(&https_to_closed, giving()).await,
        failing_through(&http_to_tls, giving()).await,
        failing_through(
            &plain.url(),
            Counting::failing(CredentialFailure::fetch_failed("the vault is down")),
        )
        .await,
        failing_through(
            &plain.url(),
            Counting::failing(CredentialFailure::no_usable("no token yet")),
        )
        .await,
        failing_through(&plain.url(), Arc::new(Raw(format!("{TOKEN}\n")))).await,
        failing_through(&plain.url(), Arc::new(Raw(String::new()))).await,
    ];
    assert!(
        matches!(errors[0], Error::Unavailable(_)),
        "{:?}",
        errors[0]
    );
    assert!(matches!(errors[5], Error::Credential(_)), "{:?}", errors[5]);
    for error in &errors {
        let text = rendered(error);
        assert!(!text.contains(TOKEN), "a token leaked in {text}");
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn no_text_of_a_failed_bus_request_holds_the_token() {
    let server = nats::Server::start();
    let service = server.client().await;
    let mut mute = service.subscribe("svc.mute").await.expect("subscribed");
    service.flush().await.expect("flushed");
    let connection = Connection::new(
        "http://127.0.0.1:1/graphql",
        Counting::giving_token(),
        server.client().await,
        account(),
        Duration::from_secs(10),
    )
    .expect("builds");
    let mut errors = Vec::new();
    for (subject, wait) in [
        ("svc.nobody", Duration::from_secs(5)),
        ("svc.mute", Duration::from_millis(200)),
        ("a b", Duration::from_secs(5)),
        ("svc.nobody", Duration::ZERO),
    ] {
        errors.push(
            connection
                .bus_request(subject, Bytes::from_static(b"x"), wait)
                .await
                .expect_err("the request fails"),
        );
    }
    assert!(mute.next().await.is_some());
    for error in &errors {
        let text = rendered(error);
        assert!(!text.contains(TOKEN), "a token leaked in {text}");
    }
}

#[tokio::test]
async fn no_debug_form_of_a_connection_a_credential_or_a_token_holds_the_token() {
    let server = HttpServer::start(serve(|_| data(json!({})))).await;
    let connection = connection_to(
        &server.url(),
        std::sync::Arc::new(FixedToken::new(TOKEN)),
        Duration::from_secs(10),
    )
    .await;
    connection.graphql("{ x }", json!({})).await.expect("data");
    let forms = [
        format!("{connection:?}"),
        format!("{connection:#?}"),
        format!("{:?}", FixedToken::new(TOKEN)),
        format!("{:?}", Token::new(TOKEN)),
    ];
    for form in forms {
        assert!(!form.contains(TOKEN), "{form}");
    }
}
