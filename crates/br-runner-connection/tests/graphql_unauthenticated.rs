mod support;

use br_runner_connection::{Cause, Error};
use serde_json::{Value, json};
use support::*;

async fn with_status(status: u16, body: Value) -> Result<Value, Error> {
    answered(move || Action::json(status, body.clone())).await
}

#[tokio::test]
async fn unauthenticated_is_judged_by_its_code_whatever_its_reason_params_or_status() {
    let malformed = [
        json!({ "code": "UNAUTHENTICATED", "reason": 5 }),
        json!({ "code": "UNAUTHENTICATED", "reason": { "a": 1 } }),
        json!({ "code": "UNAUTHENTICATED", "params": [] }),
        json!({ "code": "UNAUTHENTICATED", "params": "x" }),
        json!({ "code": "UNAUTHENTICATED", "reason": 5, "params": 7 }),
    ];
    for extensions in malformed {
        for status in [200, 400, 403, 404, 500] {
            let body = json!({ "errors": [{ "message": "m", "extensions": extensions }] });
            let got = with_status(status, body).await;
            assert!(
                matches!(got, Err(Error::Unauthenticated)),
                "{extensions} on {status}: {got:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_malformed_unauthenticated_error_wins_over_a_well_formed_one_before_it() {
    for status in [200, 500] {
        let body = json!({ "errors": [
            { "extensions": { "code": "FOO" } },
            { "extensions": { "code": "UNAUTHENTICATED", "reason": 5 } }
        ] });
        let got = with_status(status, body).await;
        assert!(
            matches!(got, Err(Error::Unauthenticated)),
            "{status}: {got:?}"
        );
    }
}

#[tokio::test]
async fn a_malformed_error_of_another_code_is_unreadable_on_a_2xx_and_lost_on_a_500() {
    let body = json!({ "errors": [{ "extensions": { "code": "FOO", "reason": 5 } }] });
    let got = with_status(200, body.clone()).await;
    assert!(matches!(got, Err(Error::Unreadable(_))), "{got:?}");
    match with_status(500, body).await {
        Err(Error::Lost(failure)) => assert_eq!(failure.cause, Cause::Transport),
        other => panic!("expected Lost, got {other:?}"),
    }
}

#[tokio::test]
async fn the_code_must_be_the_exact_text_and_stand_in_an_extensions_object() {
    let not_it = [
        json!({ "extensions": { "code": "unauthenticated" } }),
        json!({ "extensions": { "code": 7 } }),
        json!({ "extensions": ["UNAUTHENTICATED"] }),
        json!({ "message": "UNAUTHENTICATED" }),
        json!({ "code": "UNAUTHENTICATED" }),
    ];
    for error in not_it {
        let got = with_status(200, json!({ "errors": [error] })).await;
        assert!(
            !matches!(got, Err(Error::Unauthenticated)),
            "{error}: {got:?}"
        );
    }
}

#[tokio::test]
async fn errors_that_are_not_a_list_do_not_make_an_unauthenticated() {
    let got = with_status(
        200,
        json!({ "errors": { "extensions": { "code": "UNAUTHENTICATED" } } }),
    )
    .await;
    assert!(matches!(got, Err(Error::Unreadable(_))), "{got:?}");
}
