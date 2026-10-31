mod support;

use br_graph_contract::CancelToken;
use br_runner_as_tool::{PayloadKind, ToolMode};
use br_runner_jobs::test_double::{Answer, memory_caller};
use serde_json::json;
use support::*;

fn cancelled_text() -> Vec<String> {
    vec!["The call to the runner analyst was cancelled.".to_owned()]
}

#[tokio::test]
async fn given_a_kept_payload_and_a_created_job_when_the_view_triggers_then_the_cancel_goes_after_the_upload_and_the_create()
 {
    let (caller, script) = memory_caller();
    script.cancel(Answer::Never);
    let tool = builder("analyst", ToolMode::Sync, caller)
        .payload_kind(PayloadKind::Kept)
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({}));
    create_of(&script).await;
    assert!(call.pending().await);

    call.cancel.trigger();
    let outcome = call.finished().await.expect("an expected error");

    assert_eq!(texts(&outcome), cancelled_text());
    assert_eq!(kinds(&script), vec!["wait", "upload", "create", "cancel"]);
}

#[tokio::test]
async fn given_a_kept_payload_and_a_view_triggered_before_the_call_when_called_then_no_upload_is_handed_over()
 {
    let (caller, script) = memory_caller();
    let tool = builder("analyst", ToolMode::Sync, caller)
        .payload_kind(PayloadKind::Kept)
        .build()
        .expect("a tool");
    let token = CancelToken::new();
    token.handle().trigger();

    let outcome = spawn_with_cancel(&tool, json!({}), &token)
        .finished()
        .await
        .expect("an expected error");

    assert_eq!(texts(&outcome), cancelled_text());
    assert!(
        script
            .calls()
            .iter()
            .all(|call| !is_upload(call) && !is_create(call))
    );
}

#[tokio::test]
async fn given_a_background_tool_and_a_view_triggered_before_the_call_when_called_then_nothing_is_sent()
 {
    let (caller, script) = memory_caller();
    let tool = background_tool(caller, id(77));
    let token = CancelToken::new();
    token.handle().trigger();

    let outcome = spawn_with_cancel(&tool, json!({}), &token)
        .finished()
        .await
        .expect("an expected error");

    assert!(outcome.is_error());
    assert!(script.calls().is_empty());
}

#[tokio::test]
async fn given_a_background_call_that_returned_when_the_view_triggers_later_then_nothing_is_cancelled()
 {
    let (caller, script) = memory_caller();
    let tool = background_tool(caller, id(77));
    let token = CancelToken::new();
    let call = spawn_with_cancel(&tool, json!({}), &token);
    let outcome = call.finished().await.expect("an acknowledgement");
    token.handle().trigger();
    settle().await;

    assert!(!outcome.is_error());
    assert_eq!(kinds(&script), vec!["create"]);
}
