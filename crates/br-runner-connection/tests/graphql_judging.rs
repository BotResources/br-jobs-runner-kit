mod support;

use std::time::Duration;

use br_runner_connection::{Cause, CodeOrigin, Error};
use serde_json::{Value, json};
use support::*;

async fn answered(action: impl Fn() -> Action + Send + Sync + 'static) -> Result<Value, Error> {
    let server = HttpServer::start(serve(move |_| action())).await;
    let connection = connection_to(
        &server.url(),
        Counting::giving_token(),
        Duration::from_secs(10),
    )
    .await;
    connection.graphql("{ x }", json!({})).await
}

fn refused(result: Result<Value, Error>) -> br_runner_connection::Refusal {
    match result {
        Err(Error::Refused(refusal)) => refusal,
        other => panic!("expected Refused, got {other:?}"),
    }
}

#[tokio::test]
async fn a_2xx_with_data_and_no_error_gives_the_data() {
    let got = answered(|| Action::json(200, json!({ "data": { "a": 1 }, "errors": [] }))).await;
    assert_eq!(got.expect("data"), json!({ "a": 1 }));
}

#[tokio::test]
async fn an_error_code_is_given_with_its_params_and_message_and_the_data_beside_it_is_dropped() {
    let got = answered(|| {
        Action::json(
            200,
            json!({
                "data": { "a": 1 },
                "errors": [{
                    "message": "no way",
                    "extensions": { "code": "CONFLICT", "reason": "taken", "params": { "name": "Acme" } }
                }]
            }),
        )
    })
    .await;
    let refusal = refused(got);
    assert_eq!(refusal.code, "CONFLICT");
    assert_eq!(refusal.message, "no way");
    assert_eq!(refusal.origin, CodeOrigin::Service);
    assert_eq!(refusal.params.get("reason"), Some(&json!("taken")));
    assert_eq!(
        refusal.params.get("params"),
        Some(&json!({ "name": "Acme" }))
    );
    assert!(!refusal.params.contains_key("code"));
}

#[tokio::test]
async fn with_several_errors_the_first_one_that_has_a_code_is_given() {
    let got = answered(|| {
        Action::json(
            200,
            json!({ "errors": [
                { "message": "no code here" },
                { "message": "b", "extensions": { "code": "SECOND" } },
                { "message": "c", "extensions": { "code": "THIRD" } }
            ] }),
        )
    })
    .await;
    assert_eq!(refused(got).code, "SECOND");
}

#[tokio::test]
async fn unauthenticated_wins_over_refused_wherever_it_stands_in_the_list() {
    for status in [200, 400, 500] {
        let got = answered(move || {
            Action::json(
                status,
                json!({ "errors": [
                    { "extensions": { "code": "CONFLICT" } },
                    { "extensions": { "code": "UNAUTHENTICATED" } }
                ] }),
            )
        })
        .await;
        assert!(
            matches!(got, Err(Error::Unauthenticated)),
            "status {status}: {got:?}"
        );
    }
}

#[tokio::test]
async fn an_http_401_is_unauthenticated_whatever_its_body() {
    for body in [
        Vec::new(),
        b"not json at all".to_vec(),
        serde_json::to_vec(&json!({ "errors": [{ "extensions": { "code": "FORBIDDEN" } }] }))
            .expect("json"),
        serde_json::to_vec(&json!({ "data": { "a": 1 } })).expect("json"),
    ] {
        let got = answered(move || Action::Reply(401, body.clone())).await;
        assert!(matches!(got, Err(Error::Unauthenticated)), "{got:?}");
    }
}

#[tokio::test]
async fn an_http_403_with_no_readable_code_is_refused_forbidden_named_by_the_connection() {
    for body in [
        Vec::new(),
        b"<html>no</html>".to_vec(),
        b"{\"errors\":[{\"message\":\"x\"}]}".to_vec(),
    ] {
        let got = answered(move || Action::Reply(403, body.clone())).await;
        let refusal = refused(got);
        assert_eq!(refusal.code, "FORBIDDEN");
        assert_eq!(refusal.origin, CodeOrigin::Connection);
    }
}

#[tokio::test]
async fn an_http_403_with_a_readable_code_is_refused_as_the_response_says() {
    let got = answered(|| {
        Action::json(
            403,
            json!({ "errors": [{ "message": "m", "extensions": { "code": "NOT_YOURS" } }] }),
        )
    })
    .await;
    let refusal = refused(got);
    assert_eq!(refusal.code, "NOT_YOURS");
    assert_eq!(refusal.origin, CodeOrigin::Service);
}

#[tokio::test]
async fn a_non_2xx_status_with_a_readable_code_is_refused_as_the_response_says() {
    for status in [400, 404, 422, 500, 503] {
        let got = answered(move || {
            Action::json(
                status,
                json!({ "errors": [{ "extensions": { "code": "BAD_INPUT" } }] }),
            )
        })
        .await;
        assert_eq!(refused(got).code, "BAD_INPUT", "status {status}");
    }
}

#[tokio::test]
async fn a_2xx_that_cannot_be_read_or_holds_an_error_without_a_code_is_unreadable() {
    let unreadable: Vec<Behaviour> = vec![
        Box::new(|| Action::Reply(200, b"not json".to_vec())),
        Box::new(|| Action::Reply(200, Vec::new())),
        Box::new(|| Action::json(200, json!({}))),
        Box::new(|| Action::json(200, json!({ "data": null }))),
        Box::new(|| Action::json(200, json!({ "errors": [{ "message": "boom" }] }))),
        Box::new(|| {
            Action::json(
                200,
                json!({ "data": { "a": 1 }, "errors": [{ "message": "boom" }] }),
            )
        }),
        Box::new(|| Action::json(200, json!({ "errors": [{ "extensions": { "code": 7 } }] }))),
        Box::new(|| Action::json(200, json!({ "errors": "boom" }))),
    ];
    for action in unreadable {
        let got = answered(action).await;
        assert!(matches!(got, Err(Error::Unreadable(_))), "{got:?}");
    }
}

#[tokio::test]
async fn a_non_2xx_status_other_than_401_and_403_without_a_readable_code_is_lost() {
    for (status, body) in [
        (500, Vec::new()),
        (502, b"<html>bad gateway</html>".to_vec()),
        (404, b"{\"errors\":[{\"message\":\"nf\"}]}".to_vec()),
        (302, Vec::new()),
    ] {
        let got = answered(move || Action::Reply(status, body.clone())).await;
        match got {
            Err(Error::Lost(failure)) => assert_eq!(failure.cause, Cause::Transport),
            other => panic!("status {status}: expected Lost, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn the_judgment_never_reads_a_message_text() {
    let got = answered(|| {
        Action::json(200, json!({ "errors": [{ "message": "UNAUTHENTICATED", "extensions": { "code": "OTHER" } }] }))
    })
    .await;
    assert_eq!(refused(got).code, "OTHER");
    let got =
        answered(|| Action::json(200, json!({ "errors": [{ "message": "UNAUTHENTICATED" }] })))
            .await;
    assert!(matches!(got, Err(Error::Unreadable(_))), "{got:?}");
}
