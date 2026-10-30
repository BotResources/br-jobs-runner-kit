mod support;

use std::time::Duration;

use br_runner_connection::{Error, FixedToken, Token};
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
                        "extensions": { "code": format!("CODE_{TOKEN}"), "params": { "k": e.clone(), "list": [e.clone()] }, "reason": e.clone() }
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
        refusal.params.get("params"),
        Some(
            &json!({ "k": "echo of [redacted] in the text", "list": ["echo of [redacted] in the text"] })
        )
    );
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
