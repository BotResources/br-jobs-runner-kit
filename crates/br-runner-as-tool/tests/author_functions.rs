mod support;

use br_agentic_contract::ToolBlock;
use br_runner_as_tool::{AuthorFailure, Payload, PayloadKind, ToolMode};
use br_runner_jobs::test_double::{Answer, MemoryCall, memory_caller};
use br_runner_jobs::{Content, PayloadForm};
use serde_json::json;
use support::*;

#[tokio::test]
async fn given_a_payload_builder_when_called_then_its_content_replaces_the_arguments_whole() {
    let (caller, script) = memory_caller();
    let tool = builder("analyst", ToolMode::Sync, caller)
        .payload_builder(|arguments| async move {
            let text = arguments.to_json_string().expect("text");
            Ok(Payload::Content(Content {
                message: format!("snapshot + {text}"),
                attachments: Vec::new(),
            }))
        })
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({ "q": 1 }));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("ok")));
    call.finished().await.expect("a tool result");

    assert_eq!(
        create.payload,
        PayloadForm::Inline(Content {
            message: r#"snapshot + {"q":1}"#.to_owned(),
            attachments: Vec::new()
        })
    );
}

#[tokio::test]
async fn given_a_payload_kept_by_jobs_when_called_then_the_upload_goes_before_the_create_with_the_same_id()
 {
    let (caller, script) = memory_caller();
    let tool = builder("analyst", ToolMode::Sync, caller)
        .payload_kind(PayloadKind::Kept)
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({ "q": 2 }));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("ok")));
    call.finished().await.expect("a tool result");

    assert_eq!(create.payload, PayloadForm::Kept);
    assert_eq!(kinds(&script), vec!["wait", "upload", "create", "finish"]);
    let MemoryCall::Upload { job_id, content } = script.calls()[1].clone() else {
        panic!("an upload");
    };
    assert_eq!(job_id, create.job_id);
    assert_eq!(content.message, r#"{"q":2}"#);
}

#[tokio::test]
async fn given_a_reference_from_the_builder_when_called_then_it_replaces_the_form_and_nothing_is_uploaded()
 {
    let (caller, script) = memory_caller();
    let tool = builder("analyst", ToolMode::Sync, caller)
        .payload_kind(PayloadKind::Kept)
        .payload_builder(|_| async {
            Ok(Payload::Reference {
                service: "documents".to_owned(),
                document_id: "doc-9".to_owned(),
            })
        })
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("ok")));
    call.finished().await.expect("a tool result");

    assert_eq!(
        create.payload,
        PayloadForm::Reference {
            service: "documents".to_owned(),
            document_id: "doc-9".to_owned()
        }
    );
    assert_eq!(count(&script, is_upload), 0);
}

#[tokio::test]
async fn given_a_builder_that_refuses_when_called_then_an_error_flag_with_its_text_and_nothing_is_sent()
 {
    for mode in [
        ToolMode::Sync,
        ToolMode::Background {
            caller_address: id(8),
        },
    ] {
        let (caller, script) = memory_caller();
        let tool = builder("analyst", mode, caller)
            .payload_kind(PayloadKind::Kept)
            .payload_builder(|_| async { Err(AuthorFailure::refused("the ticket is closed")) })
            .build()
            .expect("a tool");

        let outcome = spawn_call(&tool, json!({}))
            .finished()
            .await
            .expect("an expected error");

        assert!(outcome.is_error());
        assert_eq!(texts(&outcome), vec!["the ticket is closed".to_owned()]);
        assert!(
            script
                .calls()
                .iter()
                .all(|call| matches!(call, MemoryCall::EndWait { .. }))
                || script.calls().is_empty()
        );
        assert_eq!(count(&script, is_create), 0);
        assert_eq!(count(&script, is_upload), 0);
    }
}

#[tokio::test]
async fn given_a_builder_that_faults_when_called_then_a_fault_with_its_error_as_cause() {
    let (caller, script) = memory_caller();
    let tool = builder("analyst", ToolMode::Sync, caller)
        .payload_builder(|_| async {
            Err(AuthorFailure::fault(std::io::Error::other(
                "the snapshot is gone",
            )))
        })
        .build()
        .expect("a tool");

    let fault = spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect_err("a fault");

    let cause = fault
        .cause()
        .downcast_ref::<std::io::Error>()
        .expect("the io error");
    assert_eq!(cause.to_string(), "the snapshot is gone");
    assert_eq!(count(&script, is_create), 0);
}

#[tokio::test]
async fn given_a_result_processor_when_completed_then_its_blocks_replace_the_default_content() {
    let (caller, script) = memory_caller();
    let tool = builder("analyst", ToolMode::Sync, caller)
        .result_processor(|result| async move {
            Ok(vec![
                ToolBlock::text(format!("summary of: {}", result.message)).expect("a block"),
                ToolBlock::text("second").expect("a block"),
            ])
        })
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("raw answer")));

    let outcome = call.finished().await.expect("a tool result");

    assert!(!outcome.is_error());
    assert_eq!(
        texts(&outcome),
        vec!["summary of: raw answer".to_owned(), "second".to_owned()]
    );
    assert_eq!(kinds(&script), vec!["wait", "create", "finish"]);
}

#[tokio::test]
async fn given_a_result_processor_that_refuses_or_faults_when_completed_then_an_error_flag_or_a_fault()
 {
    let (caller, script) = memory_caller();
    let tool = builder("analyst", ToolMode::Sync, caller)
        .result_processor(|_| async { Err(AuthorFailure::refused("the answer has no table")) })
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("raw")));
    let outcome = call.finished().await.expect("an expected error");
    assert!(outcome.is_error());
    assert_eq!(texts(&outcome), vec!["the answer has no table".to_owned()]);
    assert_eq!(count(&script, is_finish), 1);

    let (caller, script) = memory_caller();
    let tool = builder("analyst", ToolMode::Sync, caller)
        .result_processor(|_| async {
            Err(AuthorFailure::fault(std::io::Error::other(
                "the parser broke",
            )))
        })
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("raw")));
    let fault = call.finished().await.expect_err("a fault");
    assert!(fault.cause().downcast_ref::<std::io::Error>().is_some());
    assert_eq!(count(&script, is_cancel), 0);
}

#[tokio::test]
async fn given_a_result_processor_and_a_failed_child_job_when_settled_then_the_processor_is_not_run()
 {
    let (caller, script) = memory_caller();
    let tool = builder("analyst", ToolMode::Sync, caller)
        .result_processor(|_| async { Err(AuthorFailure::refused("must not run")) })
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(
        create.job_id,
        Answer::Now(br_runner_jobs::SyncEnd::Failed {
            report: None,
            message: "boom".to_owned(),
        }),
    );
    let outcome = call.finished().await.expect("an expected error");
    assert_eq!(
        texts(&outcome),
        vec!["The runner analyst failed.\nboom".to_owned()]
    );
}
