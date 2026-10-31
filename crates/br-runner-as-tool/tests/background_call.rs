mod support;

use br_runner_as_tool::{PayloadKind, ToolMode};
use br_runner_jobs::test_double::{Answer, memory_caller};
use br_runner_jobs::{CallMode, Error, PayloadForm, Removal};
use serde_json::json;
use support::*;

#[tokio::test]
async fn given_a_background_tool_when_called_then_the_create_carries_the_address_and_an_acknowledgement_returns_at_once()
 {
    let (caller, script) = memory_caller();
    let tool = background_tool(caller, id(77));

    let outcome = spawn_call(&tool, json!({ "q": 1 }))
        .finished()
        .await
        .expect("an acknowledgement");

    assert!(!outcome.is_error());
    let create = create_of(&script).await;
    assert_eq!(
        create.mode,
        CallMode::Background {
            caller_address: id(77)
        }
    );
    assert_eq!(create.runner_type, "analyst");
    let text = texts(&outcome);
    assert_eq!(text.len(), 1);
    assert!(text[0].contains("analyst"));
    assert!(text[0].contains(&create.job_id.to_string()));
    assert!(text[0].contains("not confirmed until the notice \"accepted\""));
    assert_eq!(kinds(&script), vec!["create"]);
}

#[tokio::test]
async fn given_a_background_tool_when_called_then_no_wait_no_finish_no_cancel_and_no_read() {
    let (caller, script) = memory_caller();
    let tool = background_tool(caller, id(77));
    spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect("an acknowledgement");
    settle().await;
    assert_eq!(kinds(&script), vec!["create"]);
}

#[tokio::test]
async fn given_a_create_and_an_upload_that_never_end_when_called_in_background_then_the_call_still_returns()
 {
    let (caller, script) = memory_caller();
    script.upload(Answer::Never);
    script.create(Answer::Never);
    let tool = builder(
        "analyst",
        ToolMode::Background {
            caller_address: id(77),
        },
        caller,
    )
    .payload_kind(PayloadKind::Kept)
    .build()
    .expect("a tool");

    let outcome = spawn_call(&tool, json!({ "q": 3 }))
        .finished()
        .await
        .expect("an acknowledgement");

    assert!(!outcome.is_error());
    assert_eq!(kinds(&script), vec!["upload", "create"]);
}

#[tokio::test]
async fn given_a_refused_create_when_called_in_background_then_the_library_does_not_hear_it() {
    let (caller, script) = memory_caller();
    script.create(Answer::Now(Err(Removal::Refused(refused("busy").await))));
    let tool = background_tool(caller, id(77));
    let outcome = spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect("an acknowledgement");
    assert!(!outcome.is_error());
    assert_eq!(count(&script, is_cancel), 0);
}

#[tokio::test]
async fn given_a_kept_payload_when_called_in_background_then_the_upload_goes_before_the_create() {
    let (caller, script) = memory_caller();
    let tool = builder(
        "analyst",
        ToolMode::Background {
            caller_address: id(77),
        },
        caller,
    )
    .payload_kind(PayloadKind::Kept)
    .build()
    .expect("a tool");
    spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect("an acknowledgement");
    assert_eq!(kinds(&script), vec!["upload", "create"]);
    let create = create_of(&script).await;
    assert_eq!(create.payload, PayloadForm::Kept);
}

#[tokio::test]
async fn given_a_handle_that_cannot_take_the_upload_or_the_create_when_called_in_background_then_a_fault_and_no_acknowledgement()
 {
    let (caller, script) = memory_caller();
    script.upload(Answer::Fail(Error::Unauthenticated));
    let tool = builder(
        "analyst",
        ToolMode::Background {
            caller_address: id(77),
        },
        caller,
    )
    .payload_kind(PayloadKind::Kept)
    .build()
    .expect("a tool");
    let fault = spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect_err("a fault");
    assert_eq!(
        fault.cause().downcast_ref::<Error>(),
        Some(&Error::Unauthenticated)
    );
    assert_eq!(kinds(&script), vec!["upload"]);

    let (caller, script) = memory_caller();
    script.create(Answer::Fail(Error::Unauthenticated));
    let tool = background_tool(caller, id(77));
    let fault = spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect_err("a fault");
    assert_eq!(
        fault.cause().downcast_ref::<Error>(),
        Some(&Error::Unauthenticated)
    );
    assert_eq!(kinds(&script), vec!["create"]);
}

#[tokio::test]
async fn given_a_result_processor_and_the_keep_flag_when_called_in_background_then_they_have_no_effect()
 {
    let (caller, script) = memory_caller();
    let tool = builder(
        "analyst",
        ToolMode::Background {
            caller_address: id(77),
        },
        caller,
    )
    .keep_result(true)
    .result_processor(|_| async { Err(br_runner_as_tool::AuthorFailure::refused("never run")) })
    .build()
    .expect("a tool");
    let outcome = spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect("an acknowledgement");
    assert!(!outcome.is_error());
    assert_eq!(kinds(&script), vec!["create"]);
}

#[tokio::test]
async fn given_two_background_calls_when_called_then_each_has_its_own_child_job_id_and_command_id()
{
    let (caller, script) = memory_caller();
    let tool = background_tool(caller, id(77));
    let first = spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect("an acknowledgement");
    let second = spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect("an acknowledgement");
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
    assert_ne!(texts(&first), texts(&second));
}
