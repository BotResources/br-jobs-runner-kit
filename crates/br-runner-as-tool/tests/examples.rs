mod support;

#[path = "../examples/background_tool.rs"]
#[allow(dead_code)]
mod background_tool;
#[path = "../examples/builder_and_processor.rs"]
#[allow(dead_code)]
mod builder_and_processor;
#[path = "../examples/sync_tool.rs"]
#[allow(dead_code)]
mod sync_tool;

use br_runner_jobs::test_double::{Answer, memory_caller};
use br_runner_jobs::{CallMode, PayloadForm};
use serde_json::json;
use support::*;

#[tokio::test]
async fn given_the_sync_example_when_the_model_calls_it_then_the_analyst_answer_comes_back() {
    let (caller, script) = memory_caller();
    let tool = sync_tool::analyst_tool(caller).expect("a tool");
    assert_eq!(tool.name().as_str(), "ask_analyst");
    let call = spawn_call(&tool, json!({ "question": "how many tickets" }));
    let create = create_of(&script).await;
    script.end(create.job_id, Answer::Now(completed("42 tickets")));

    let outcome = call.finished().await.expect("a tool result");

    assert_eq!(texts(&outcome), vec!["42 tickets".to_owned()]);
    assert_eq!(create.runner_type, "analyst");
    assert_eq!(create.mode, CallMode::Sync);
}

#[tokio::test]
async fn given_the_background_example_when_the_model_calls_it_then_an_acknowledgement_comes_at_once()
 {
    let (caller, script) = memory_caller();
    let tool = background_tool::report_writer_tool(caller, id(31)).expect("a tool");

    let outcome = spawn_call(&tool, json!({ "topic": "the year" }))
        .finished()
        .await
        .expect("an acknowledgement");

    let create = create_of(&script).await;
    assert!(!outcome.is_error());
    assert_eq!(create.runner_type, "report-writer");
    assert_eq!(
        create.mode,
        CallMode::Background {
            caller_address: id(31)
        }
    );
    assert_eq!(create.payload, PayloadForm::Kept);
    assert_eq!(kinds(&script), vec!["upload", "create"]);
}

#[tokio::test]
async fn given_the_builder_example_when_called_then_the_snapshot_joins_the_payload_and_the_summary_is_prefixed()
 {
    let (caller, script) = memory_caller();
    let tool =
        builder_and_processor::summary_tool(caller, "state: step 3".to_owned()).expect("a tool");
    let call = spawn_call(&tool, json!({}));
    let create = create_of(&script).await;
    script.end(
        create.job_id,
        Answer::Now(completed("  three steps done  ")),
    );

    let outcome = call.finished().await.expect("a tool result");

    assert_eq!(
        texts(&outcome),
        vec!["Summary: three steps done".to_owned()]
    );
    assert_eq!(
        create.payload,
        PayloadForm::Inline(content("state: step 3\n{}"))
    );
}

#[tokio::test]
async fn given_the_builder_example_with_an_empty_snapshot_when_called_then_the_agent_is_told_and_nothing_is_created()
 {
    let (caller, script) = memory_caller();
    let tool = builder_and_processor::summary_tool(caller, String::new()).expect("a tool");

    let outcome = spawn_call(&tool, json!({}))
        .finished()
        .await
        .expect("an expected error");

    assert!(outcome.is_error());
    assert_eq!(
        texts(&outcome),
        vec!["There is nothing to summarize yet.".to_owned()]
    );
    assert_eq!(count(&script, is_create), 0);
}
