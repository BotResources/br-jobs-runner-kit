mod support;

use br_runner_jobs::test_double::{Call, ScriptedJobs};
use br_runner_jobs::{Content, FailureKind};
use serde_json::{Value, json};
use support::*;

fn sync_trigger(n: u128) -> Vec<u8> {
    call_with(trigger(n), &[("mode", json!("SYNC"))])
}

fn graphql_calls(jobs: &ScriptedJobs, root: &str) -> Vec<Value> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Graphql {
                document,
                variables,
            } if document.contains(root) => Some(variables),
            _ => None,
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn given_a_sync_call_when_the_graph_ends_then_the_answer_is_the_fields_as_text_and_the_run_completes()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(writes_ok());
    jobs.push_trigger(sync_trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let uploads = graphql_calls(&jobs, "jobsUploadResult");
    assert_eq!(uploads.len(), 1);
    assert_eq!(
        uploads[0]["input"]["answer"],
        json!({ "message": "{\"steps\":[\"x\"]}", "attachments": [] })
    );
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
}

#[tokio::test(start_paused = true)]
async fn given_an_answer_function_when_a_sync_call_ends_then_its_content_is_the_answer() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(writes_ok());
    jobs.push_trigger(sync_trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::Result("x")), false).with_answer(|state| {
            Ok(Content {
                message: format!("{} step", state.fields().steps.len()),
                attachments: Vec::new(),
            })
        }),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let uploads = graphql_calls(&jobs, "jobsUploadResult");
    assert_eq!(uploads[0]["input"]["answer"]["message"], json!("1 step"));
}

#[tokio::test(start_paused = true)]
async fn given_an_answer_function_that_refuses_when_a_sync_call_ends_then_the_run_fails_with_answer_not_made()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(writes_ok());
    jobs.push_trigger(sync_trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::Result("x")), false).with_answer(|_| Err("no answer".to_owned())),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let failed = failed_fact(&jobs).expect("failed");
    assert_eq!(failed.reason_code, "answer_not_made");
    assert_eq!(failed.kind, FailureKind::Permanent);
    assert!(graphql_calls(&jobs, "jobsUploadResult").is_empty());
    assert_eq!(kinds(&jobs), vec!["started", "failed"]);
}

#[tokio::test(start_paused = true)]
async fn given_an_answer_function_that_panics_when_a_sync_call_ends_then_the_run_fails_with_panic()
{
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(writes_ok());
    jobs.push_trigger(sync_trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::Result("x")), false).with_answer(|_| panic!("the answer panics")),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_eq!(failed_fact(&jobs).expect("failed").reason_code, "panic");
}

#[tokio::test(start_paused = true)]
async fn given_a_call_that_is_not_sync_when_the_graph_ends_then_no_answer_is_uploaded() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(writes_ok());
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert!(graphql_calls(&jobs, "jobsUploadResult").is_empty());
}

#[tokio::test(start_paused = true)]
async fn given_a_refused_answer_upload_when_the_run_completes_then_the_completed_fact_is_removed_and_the_run_fails_with_answer_refused()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(json!({
        "jobsUploadResult": { "success": false, "reason": "too_big", "message": "the answer is too big" }
    }));
    jobs.push_trigger(sync_trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let failed = failed_fact(&jobs).expect("failed");
    assert_eq!(failed.reason_code, "answer_refused");
    assert_eq!(failed.kind, FailureKind::Permanent);
    assert!(
        !facts(&jobs)
            .iter()
            .any(|fact| matches!(fact, br_runner_jobs::test_double::Fact::Completed { .. }))
    );
}
