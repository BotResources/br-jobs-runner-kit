mod support;

use br_agentic_contract::ToolBlock;
use br_graph_contract::CancelToken;
use br_runner_as_tool::{AuthorFailure, Payload, PayloadKind, ToolMode};
use br_runner_jobs::SyncEnd;
use br_runner_jobs::test_double::{Answer, MemoryCall, memory_caller, result_ref};
use serde_json::json;
use support::*;

fn cancelled_text() -> Vec<String> {
    vec!["The call to the runner analyst was cancelled.".to_owned()]
}

#[tokio::test]
async fn given_a_view_triggered_before_the_call_when_called_then_no_create_and_no_cancel() {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let token = CancelToken::new();
    token.handle().trigger();

    let outcome = spawn_with_cancel(&tool, json!({}), &token)
        .finished()
        .await
        .expect("an expected error");

    assert!(outcome.is_error());
    assert_eq!(texts(&outcome), cancelled_text());
    assert_eq!(count(&script, is_create), 0);
    assert_eq!(count(&script, is_cancel), 0);
    assert_eq!(count(&script, is_upload), 0);
}

#[tokio::test]
async fn given_a_payload_builder_that_never_answers_when_the_view_triggers_then_the_call_ends_with_no_create_and_no_cancel()
 {
    for kind in [PayloadKind::Inline, PayloadKind::Kept] {
        let (caller, script) = memory_caller();
        let tool = builder("analyst", ToolMode::Sync, caller)
            .payload_kind(kind)
            .payload_builder(|_| std::future::pending::<Result<Payload, AuthorFailure>>())
            .build()
            .expect("a tool");
        let call = spawn_call(&tool, json!({}));
        assert!(call.pending().await);

        call.cancel.trigger();
        let outcome = call.finished().await.expect("an expected error");

        assert_eq!(texts(&outcome), cancelled_text());
        assert_eq!(count(&script, is_upload), 0);
        assert_eq!(count(&script, is_create), 0);
        assert_eq!(count(&script, is_cancel), 0);
    }
}

#[tokio::test]
async fn given_a_create_handed_over_when_the_view_triggers_then_one_cancel_goes_after_it_and_no_answer_is_awaited()
 {
    for create_answer in [Answer::delivered(), Answer::Late(Ok(())), Answer::Never] {
        let (caller, script) = memory_caller();
        script.create(create_answer);
        script.cancel(Answer::Never);
        let tool = sync_tool(caller);
        let call = spawn_call(&tool, json!({}));
        let create = create_of(&script).await;
        assert!(call.pending().await);

        call.cancel.trigger();
        let outcome = call.finished().await.expect("an expected error");

        assert_eq!(texts(&outcome), cancelled_text());
        assert_eq!(kinds(&script), vec!["wait", "create", "cancel"]);
        let cancel = script
            .calls()
            .into_iter()
            .find_map(|call| match call {
                MemoryCall::Cancel { job_id, command_id } => Some((job_id, command_id)),
                _ => None,
            })
            .expect("a cancel");
        assert_eq!(cancel.0, create.job_id);
        assert_ne!(cancel.1, create.command_id);
    }
}

#[tokio::test]
async fn given_a_cancel_that_cannot_be_handed_over_when_the_view_triggers_then_the_call_still_ends_cancelled()
 {
    let (caller, script) = memory_caller();
    script.cancel(Answer::Fail(br_runner_jobs::Error::Unauthenticated));
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    create_of(&script).await;
    call.cancel.trigger();
    let outcome = call.finished().await.expect("an expected error");
    assert_eq!(texts(&outcome), cancelled_text());
}

#[tokio::test]
async fn given_a_create_that_ended_refused_when_the_view_triggers_then_no_cancel_goes() {
    let (caller, script) = memory_caller();
    let refusal = refused_saying("job_terminal", "over").await;
    script.create(Answer::Late(Err(br_runner_jobs::Removal::Refused(refusal))));
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    create_of(&script).await;
    assert!(call.pending().await);
    script.release();
    call.cancel.trigger();

    let outcome = call.finished().await.expect("an expected error");

    assert!(outcome.is_error());
    assert_eq!(count(&script, is_cancel), 0);
}

#[tokio::test]
async fn given_the_end_of_the_sync_call_received_when_the_view_triggers_during_the_read_then_no_cancel_and_no_finish()
 {
    let (caller, script) = memory_caller();
    script.read_result(Answer::Late(content("late")));
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(
        create.job_id,
        Answer::Now(SyncEnd::Completed {
            inline: None,
            result_ref: Some(result_ref(id(4))),
        }),
    );
    for _ in 0..10_000 {
        if count(&script, is_read) == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(call.pending().await);

    call.cancel.trigger();
    let outcome = call.finished().await.expect("an expected error");

    assert_eq!(texts(&outcome), cancelled_text());
    assert_eq!(count(&script, is_cancel), 0);
    assert_eq!(count(&script, is_finish), 0);
}

#[tokio::test]
async fn given_a_result_processor_that_never_answers_when_the_view_triggers_then_the_finish_stays_and_no_cancel_goes()
 {
    let (caller, script) = memory_caller();
    let tool = builder("analyst", ToolMode::Sync, caller)
        .result_processor(|_| std::future::pending::<Result<Vec<ToolBlock>, AuthorFailure>>())
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("raw")));
    for _ in 0..10_000 {
        if count(&script, is_finish) == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(call.pending().await);

    call.cancel.trigger();
    let outcome = call.finished().await.expect("an expected error");

    assert_eq!(texts(&outcome), cancelled_text());
    assert_eq!(kinds(&script), vec!["wait", "create", "finish"]);
}

#[tokio::test]
async fn given_a_view_triggered_after_the_end_arrived_with_an_inline_result_when_settled_then_no_cancel_is_ever_sent()
 {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let token = CancelToken::new();
    let call = spawn_with_cancel(&tool, json!({}), &token);
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("done")));
    let outcome = call.finished().await;
    token.handle().trigger();

    assert!(outcome.is_ok());
    assert_eq!(count(&script, is_cancel), 0);
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
