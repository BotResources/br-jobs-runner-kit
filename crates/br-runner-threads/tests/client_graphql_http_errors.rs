mod support;

use std::time::Duration;

use br_runner_threads::{Cause, ContextQuery, Error, SetTitle};
use serde_json::json;
use support::nats_fixtures::BEARER;
use support::nats_gateway::gateway_rig_with;
use support::nats_http::Answer;
use support::v7;

fn query() -> ContextQuery {
    ContextQuery {
        thread_id: v7(),
        agent: None,
    }
}

fn title() -> SetTitle {
    SetTitle {
        thread_id: v7(),
        title_job_id: v7(),
        text: "A title".to_owned(),
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_coded_graphql_error_is_refused_with_its_code_reason_params_and_message() {
    let body = json!({ "errors": [{
        "message": "no such thread",
        "extensions": { "code": "THREAD_NOT_FOUND", "reason": "gone", "params": { "n": 1 } },
    }]});
    let rig = gateway_rig_with(Duration::from_secs(5), move |_| {
        Answer::Json(200, body.clone())
    })
    .await;

    let result = rig.client.read_context(&query()).await;

    match result {
        Err(Error::Refused(refusal)) => {
            assert_eq!(refusal.code, "THREAD_NOT_FOUND");
            assert_eq!(refusal.reason.as_deref(), Some("gone"));
            assert_eq!(refusal.params["n"], 1);
            assert_eq!(refusal.message, "no such thread");
            assert_eq!(refusal.command_id, None);
        }
        other => panic!("Refused, not {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_rejected_credential_is_unauthenticated_and_the_token_is_in_no_text() {
    let rig = gateway_rig_with(Duration::from_secs(5), |_| Answer::Text(401, String::new())).await;

    let result = rig.client.set_title(&title()).await;

    let error = result.expect_err("refused");
    assert_eq!(error, Error::Unauthenticated);
    assert!(!format!("{error} {error:?}").contains(BEARER));
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_server_failure_is_lost_and_the_request_is_not_sent_again() {
    let rig = gateway_rig_with(Duration::from_secs(5), |_| {
        Answer::Text(500, "boom".to_owned())
    })
    .await;

    let result = rig.client.set_title(&title()).await;

    match result {
        Err(Error::Lost(failure)) => assert_eq!(failure.cause, Cause::Transport),
        other => panic!("Lost, not {other:?}"),
    }
    assert_eq!(rig.gateway.requests().len(), 1);
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn a_gateway_that_does_not_answer_within_the_reply_wait_is_lost_with_the_wait_fired() {
    let rig = gateway_rig_with(Duration::from_millis(300), |_| Answer::Hang).await;

    let result = tokio::time::timeout(Duration::from_secs(5), rig.client.read_context(&query()))
        .await
        .expect("the call ended within its wait");

    match result {
        Err(Error::Lost(failure)) => assert_eq!(failure.cause, Cause::WaitFired),
        other => panic!("Lost, not {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn an_answer_that_is_not_json_is_unreadable() {
    let rig = gateway_rig_with(Duration::from_secs(5), |_| {
        Answer::Text(200, "not json".to_owned())
    })
    .await;

    let result = rig.client.read_context(&query()).await;

    assert!(matches!(result, Err(Error::Unreadable(_))), "{result:?}");
}
