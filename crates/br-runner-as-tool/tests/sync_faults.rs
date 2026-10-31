mod support;

use br_runner_as_tool::{CreateRemoved, PayloadKind, ToolMode};
use br_runner_jobs::test_double::{Answer, memory_caller};
use br_runner_jobs::{Error, Removal, SyncEnd};
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

#[tokio::test]
async fn given_a_wait_that_fails_when_the_create_is_delivered_or_not_yet_ended_then_a_fault_and_one_cancel()
 {
    for create_answer in [Answer::delivered(), Answer::Late(Ok(())), Answer::Never] {
        let (caller, script) = memory_caller();
        script.create(create_answer);
        let tool = sync_tool(caller);
        let call = spawn_call(&tool, json!({}));
        let create = create_of(&script).await;
        script.end(
            create.job_id,
            Answer::Fail(Error::InfraMissing("the feed is gone".to_owned())),
        );

        let outcome = call.finished().await;

        assert_eq!(
            cause_error(&outcome),
            &Error::InfraMissing("the feed is gone".to_owned())
        );
        assert_eq!(kinds(&script), vec!["wait", "create", "cancel"]);
        let cancel = script
            .calls()
            .into_iter()
            .find_map(|call| match call {
                br_runner_jobs::test_double::MemoryCall::Cancel { job_id, command_id } => {
                    Some((job_id, command_id))
                }
                _ => None,
            })
            .expect("a cancel");
        assert_eq!(cancel.0, create.job_id);
        assert_ne!(cancel.1, create.command_id);
    }
}

#[tokio::test]
async fn given_a_book_that_is_dropped_when_waiting_then_a_fault_and_one_cancel() {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    create_of(&script).await;
    script.drop_book();

    let outcome = call.finished().await;

    assert_eq!(cause_error(&outcome), &Error::BookDropped);
    assert_eq!(count(&script, is_cancel), 1);
}

#[tokio::test]
async fn given_a_slow_child_job_when_nothing_arrives_then_the_call_stays_pending_and_sends_nothing_more()
 {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    create_of(&script).await;

    assert!(call.pending().await);
    assert_eq!(kinds(&script), vec!["wait", "create"]);
    drop(call);
}

#[tokio::test]
async fn given_a_result_read_that_is_refused_when_settled_then_an_error_flag_names_the_step_and_the_child_is_still_finished()
 {
    let (caller, script) = memory_caller();
    script.read_result(Answer::Fail(
        refused_saying("result_gone", "the Result was removed").await,
    ));
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(
        create.job_id,
        Answer::Now(SyncEnd::Completed {
            inline: None,
            result_ref: Some(br_runner_jobs::test_double::result_ref(id(3))),
        }),
    );

    let outcome = call.finished().await.expect("an expected error");

    assert!(outcome.is_error());
    assert_eq!(
        texts(&outcome),
        vec![
            "The Result read of the runner analyst was refused: the Result was removed".to_owned()
        ]
    );
    assert_eq!(kinds(&script), vec!["wait", "create", "read", "finish"]);
}

#[tokio::test]
async fn given_a_result_read_that_fails_otherwise_when_settled_then_a_fault_and_the_child_is_still_finished_and_not_cancelled()
 {
    let (caller, script) = memory_caller();
    script.read_result(Answer::Fail(Error::Unreadable("a Result".to_owned())));
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(
        create.job_id,
        Answer::Now(SyncEnd::Completed {
            inline: None,
            result_ref: Some(br_runner_jobs::test_double::result_ref(id(3))),
        }),
    );

    let outcome = call.finished().await;

    assert!(matches!(cause_error(&outcome), Error::Unreadable(_)));
    assert_eq!(kinds(&script), vec!["wait", "create", "read", "finish"]);
}

#[tokio::test]
async fn given_a_finish_that_never_answers_or_cannot_be_handed_over_when_settled_then_the_tool_result_is_the_same()
 {
    for answer in [
        Answer::Never,
        Answer::Fail(Error::InvalidInput("no finish".to_owned())),
        Answer::Now(Err(Removal::LeftUnsent)),
    ] {
        let (caller, script) = memory_caller();
        script.finish(answer);
        let tool = sync_tool(caller);
        let call = spawn_call(&tool, json!({}));
        let create = create_of(&script).await;
        script.end(create.job_id, Answer::Now(completed("42 tickets")));

        let outcome = call.finished().await.expect("a tool result");

        assert!(!outcome.is_error());
        assert_eq!(texts(&outcome), vec!["42 tickets".to_owned()]);
    }
}

#[tokio::test]
async fn given_a_late_end_of_the_create_when_the_end_of_the_call_arrives_first_then_the_end_settles_the_call()
 {
    let (caller, script) = memory_caller();
    script.create(Answer::Never);
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("quick")));

    let outcome = call.finished().await.expect("a tool result");

    assert_eq!(texts(&outcome), vec!["quick".to_owned()]);
    assert_eq!(kinds(&script), vec!["wait", "create", "finish"]);
}

#[tokio::test]
async fn given_parallel_calls_of_one_tool_when_one_child_is_slow_then_the_other_is_not_held_back() {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let slow = spawn_call(&tool, json!({ "n": 1 }));
    let fast = spawn_call(&tool, json!({ "n": 2 }));
    for _ in 0..10_000 {
        if count(&script, is_create) == 2 {
            break;
        }
        tokio::task::yield_now().await;
    }
    let creates: Vec<_> = script
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            br_runner_jobs::test_double::MemoryCall::Create(input) => Some(input),
            _ => None,
        })
        .collect();
    assert_eq!(creates.len(), 2);
    assert_ne!(creates[0].job_id, creates[1].job_id);
    assert_ne!(creates[0].command_id, creates[1].command_id);
    let fast_job = creates
        .iter()
        .find(|input| matches!(&input.payload, br_runner_jobs::PayloadForm::Inline(c) if c.message.contains("2")))
        .expect("the second call");
    script.end(fast_job.job_id, Answer::Now(completed("fast")));

    let outcome = fast.finished().await.expect("a tool result");

    assert_eq!(texts(&outcome), vec!["fast".to_owned()]);
    assert!(slow.pending().await);
    drop(slow);
}

#[tokio::test]
async fn given_options_of_the_author_when_the_create_is_made_then_it_carries_them_as_given() {
    let (caller, script) = memory_caller();
    let tool = builder("analyst", ToolMode::Sync, caller)
        .config(json!({ "depth": 3 }))
        .max_attempts(5)
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("ok")));
    call.finished().await.expect("a tool result");

    assert_eq!(create.config, Some(json!({ "depth": 3 })));
    assert_eq!(create.max_attempts, Some(5));
}

#[tokio::test]
async fn given_a_wait_that_fails_when_the_create_ended_refused_in_the_same_moment_then_a_fault_and_no_cancel()
 {
    let (caller, script) = memory_caller();
    let refusal = refused_saying("job_terminal", "over").await;
    script.create(Answer::Late(Err(Removal::Refused(refusal))));
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    assert!(call.pending().await);
    script.end(
        create.job_id,
        Answer::Fail(Error::InfraMissing("the feed is gone".to_owned())),
    );
    script.release();

    let outcome = call.finished().await;

    assert!(matches!(cause_error(&outcome), Error::InfraMissing(_)));
    assert_eq!(kinds(&script), vec!["wait", "create"]);
}
