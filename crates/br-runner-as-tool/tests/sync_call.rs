mod support;

use br_runner_jobs::test_double::{Answer, MemoryCall, memory_caller, result_ref};
use br_runner_jobs::{CallMode, Content, ContentAttachment, PayloadForm, SyncEnd};
use serde_json::json;
use support::*;

#[tokio::test]
async fn given_an_inline_answer_when_the_call_runs_then_the_create_is_sync_and_the_answer_is_one_text_block()
 {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({ "question": "how many tickets" }));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("42 tickets")));

    let outcome = call.finished().await.expect("a tool result");

    assert!(!outcome.is_error());
    assert_eq!(texts(&outcome), vec!["42 tickets".to_owned()]);
    assert_eq!(create.runner_type, "analyst");
    assert_eq!(create.mode, CallMode::Sync);
    assert_eq!(create.config, None);
    assert_eq!(create.max_attempts, None);
    let PayloadForm::Inline(payload) = &create.payload else {
        panic!("{:?}", create.payload);
    };
    assert_eq!(payload.message, r#"{"question":"how many tickets"}"#);
    assert_eq!(kinds(&script), vec!["wait", "create", "finish"]);
}

#[tokio::test]
async fn given_a_finished_call_when_the_finish_is_handed_over_then_it_names_the_child_job_with_a_new_command_id()
 {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("done")));
    call.finished().await.expect("a tool result");

    let finish = script
        .calls()
        .into_iter()
        .find_map(|call| match call {
            MemoryCall::Finish { job_id, command_id } => Some((job_id, command_id)),
            _ => None,
        })
        .expect("a finish");
    assert_eq!(finish.0, create.job_id);
    assert_ne!(finish.1, create.command_id);
    assert_eq!(create.job_id.get_version_num(), 7);
    assert_eq!(create.command_id.get_version_num(), 7);
}

#[tokio::test]
async fn given_a_result_by_reference_when_completed_then_it_is_read_with_the_keep_flag_of_the_author()
 {
    for keep in [false, true] {
        let (caller, script) = memory_caller();
        let tool = builder("analyst", br_runner_as_tool::ToolMode::Sync, caller)
            .keep_result(keep)
            .build()
            .expect("a tool");
        script.read_result(Answer::Now(content("from the store")));
        let call = spawn_call(&tool, json!({}));
        let create = create_of(&script).await;
        script.end(
            create.job_id,
            Answer::Now(SyncEnd::Completed {
                inline: None,
                result_ref: Some(result_ref(id(5))),
            }),
        );

        let outcome = call.finished().await.expect("a tool result");

        assert_eq!(texts(&outcome), vec!["from the store".to_owned()]);
        assert_eq!(
            script
                .calls()
                .into_iter()
                .find_map(|call| match call {
                    MemoryCall::ReadResult { result, keep } => Some((result.as_uuid(), keep)),
                    _ => None,
                })
                .expect("a read"),
            (id(5), keep)
        );
        assert_eq!(kinds(&script), vec!["wait", "create", "read", "finish"]);
    }
}

#[tokio::test]
async fn given_an_inline_result_and_the_keep_flag_when_completed_then_nothing_is_read() {
    let (caller, script) = memory_caller();
    let tool = builder("analyst", br_runner_as_tool::ToolMode::Sync, caller)
        .keep_result(true)
        .build()
        .expect("a tool");
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("inline")));
    call.finished().await.expect("a tool result");
    assert_eq!(count(&script, is_read), 0);
}

#[tokio::test]
async fn given_an_empty_result_when_completed_then_one_text_block_says_the_runner_returned_nothing()
{
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("")));

    let outcome = call.finished().await.expect("a tool result");

    assert!(!outcome.is_error());
    assert_eq!(
        texts(&outcome),
        vec!["The runner analyst returned nothing.".to_owned()]
    );
}

#[tokio::test]
async fn given_a_completed_end_with_no_result_at_all_when_settled_then_it_is_an_empty_result_and_the_child_is_finished()
 {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(
        create.job_id,
        Answer::Now(SyncEnd::Completed {
            inline: None,
            result_ref: None,
        }),
    );
    let outcome = call.finished().await.expect("a tool result");
    assert_eq!(
        texts(&outcome),
        vec!["The runner analyst returned nothing.".to_owned()]
    );
    assert_eq!(count(&script, is_finish), 1);
}

#[tokio::test]
async fn given_a_result_with_attachments_when_completed_then_each_attachment_is_a_further_text_block()
 {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(
        create.job_id,
        Answer::Now(SyncEnd::Completed {
            inline: Some(Content {
                message: "see the file".to_owned(),
                attachments: vec![ContentAttachment {
                    name: "report.csv".to_owned(),
                    content_type: "text/csv".to_owned(),
                    content: "a,b".to_owned(),
                }],
            }),
            result_ref: None,
        }),
    );
    let outcome = call.finished().await.expect("a tool result");
    assert_eq!(
        texts(&outcome),
        vec![
            "see the file".to_owned(),
            "Attachment report.csv (text/csv):\na,b".to_owned()
        ]
    );
}

#[tokio::test]
async fn given_a_json_text_result_when_completed_then_the_text_is_given_as_it_is() {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed(r#"{"count":42}"#)));
    let outcome = call.finished().await.expect("a tool result");
    assert_eq!(texts(&outcome), vec![r#"{"count":42}"#.to_owned()]);
}

#[tokio::test]
async fn given_a_failed_child_job_when_settled_then_an_error_flag_names_the_runner_and_keeps_the_message_of_jobs()
 {
    for (end, outcome_word) in [
        (
            SyncEnd::Failed {
                report: None,
                message: "the analysis crashed".to_owned(),
            },
            "failed",
        ),
        (
            SyncEnd::Cancelled {
                message: "stopped by its owner".to_owned(),
            },
            "cancelled",
        ),
    ] {
        let (caller, script) = memory_caller();
        let tool = sync_tool(caller);
        let call = spawn_call(&tool, json!({}));
        let create = create_of(&script).await;
        script.end(create.job_id, Answer::Now(end));

        let outcome = call.finished().await.expect("an expected error");

        assert!(outcome.is_error());
        let text = texts(&outcome);
        let mut lines = text[0].lines();
        assert_eq!(
            lines.next(),
            Some(format!("The runner analyst {outcome_word}.").as_str())
        );
        assert!(!text[0].contains(&create.job_id.to_string()));
        assert_eq!(kinds(&script), vec!["wait", "create"]);
    }
}

#[tokio::test]
async fn given_a_failed_child_job_when_settled_then_the_message_follows_the_first_line_unchanged() {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(
        create.job_id,
        Answer::Now(SyncEnd::Failed {
            report: None,
            message: "the analysis crashed\nsecond line".to_owned(),
        }),
    );
    let outcome = call.finished().await.expect("an expected error");
    assert_eq!(
        texts(&outcome),
        vec!["The runner analyst failed.\nthe analysis crashed\nsecond line".to_owned()]
    );
}

#[tokio::test]
async fn given_a_call_when_it_runs_then_no_fragment_goes_to_the_result_stream() {
    let (caller, script) = memory_caller();
    let tool = sync_tool(caller);
    let call = spawn_call(&tool, json!({}));
    let fragments = call.fragments.clone();
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("done")));
    call.finished().await.expect("a tool result");
    assert!(fragments.lock().expect("fragments").is_empty());
}
