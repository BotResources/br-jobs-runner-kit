mod support;

use br_runner_as_tool::{CreateRemoved, PayloadKind, ToolMode};
use br_runner_jobs::test_double::{Answer, memory_caller};
use br_runner_jobs::{Error, Removal};
use serde_json::json;
use support::*;

fn cause_error(outcome: &Outcome) -> &Error {
    outcome
        .as_ref()
        .expect_err("a fault")
        .cause()
        .downcast_ref::<Error>()
        .expect("an error of br-runner-jobs")
}

#[tokio::test]
async fn given_a_refused_create_when_waiting_then_an_error_flag_holds_the_text_of_jobs_and_no_cancel_goes()
 {
    let (caller, script) = memory_caller();
    let refusal = refused_saying("job_terminal", "the parent Job is over").await;
    script.create(Answer::Now(Err(Removal::Refused(refusal))));
    let tool = sync_tool(caller);

    let outcome = spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect("an expected error");

    assert!(outcome.is_error());
    assert_eq!(texts(&outcome), vec!["the parent Job is over".to_owned()]);
    assert_eq!(kinds(&script), vec!["wait", "create"]);
}

#[tokio::test]
async fn given_a_refusal_with_no_message_when_waiting_then_the_reason_is_the_text() {
    let (caller, script) = memory_caller();
    script.create(Answer::Now(Err(Removal::Refused(refused("busy").await))));
    let tool = sync_tool(caller);

    let outcome = spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect("an expected error");

    assert!(outcome.is_error());
    assert_eq!(texts(&outcome), vec!["busy".to_owned()]);
}

#[tokio::test]
async fn given_a_refused_upload_when_the_create_ends_refused_then_an_error_flag_and_no_cancel() {
    let (caller, script) = memory_caller();
    let refusal = refused_saying("payload_too_big", "the payload is too big").await;
    script.create(Answer::Late(Err(Removal::Refused(refusal))));
    let tool = builder("analyst", ToolMode::Sync, caller)
        .payload_kind(PayloadKind::Kept)
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({}));
    create_of(&script).await;
    assert!(call.pending().await);
    script.release();

    let outcome = call.finished().await.expect("an expected error");

    assert!(outcome.is_error());
    assert_eq!(texts(&outcome), vec!["the payload is too big".to_owned()]);
    assert_eq!(kinds(&script), vec!["wait", "upload", "create"]);
}

#[tokio::test]
async fn given_a_create_removed_after_a_report_when_waiting_then_a_fault_and_no_cancel() {
    let removals = [
        Removal::Failed(Error::InfraMissing("the command stream".to_owned())),
        Removal::DependsOnRemoved,
        Removal::LeftUnsent,
    ];
    for removal in removals {
        let (caller, script) = memory_caller();
        script.create(Answer::Now(Err(removal.clone())));
        let tool = sync_tool(caller);

        let outcome = spawn_call(&tool, json!({})).finished().await;

        let fault = outcome.expect_err("a fault");
        match removal {
            Removal::Failed(error) => {
                assert_eq!(fault.cause().downcast_ref::<Error>(), Some(&error));
            }
            other => {
                let removed = fault
                    .cause()
                    .downcast_ref::<CreateRemoved>()
                    .expect("a removed create");
                assert_eq!(removed.removal, other);
            }
        }
        assert_eq!(kinds(&script), vec!["wait", "create"]);
    }
}

#[tokio::test]
async fn given_an_upload_the_handle_cannot_take_when_called_then_a_fault_and_no_create() {
    let (caller, script) = memory_caller();
    script.upload(Answer::Fail(Error::Unauthenticated));
    let tool = builder("analyst", ToolMode::Sync, caller)
        .payload_kind(PayloadKind::Kept)
        .build()
        .expect("a tool");

    let outcome = spawn_call(&tool, json!({})).finished().await;

    assert_eq!(cause_error(&outcome), &Error::Unauthenticated);
    assert_eq!(kinds(&script), vec!["wait", "upload"]);
}

#[tokio::test]
async fn given_a_create_the_handle_cannot_take_when_called_then_a_fault_and_no_cancel_and_no_acknowledgement()
 {
    let (caller, script) = memory_caller();
    script.create(Answer::Fail(Error::Unauthenticated));
    let tool = sync_tool(caller);

    let outcome = spawn_call(&tool, json!({})).finished().await;

    assert_eq!(cause_error(&outcome), &Error::Unauthenticated);
    assert_eq!(kinds(&script), vec!["wait", "create"]);
}
