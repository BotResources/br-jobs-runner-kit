mod support;

#[path = "../examples/restartable_agent.rs"]
#[allow(dead_code)]
mod restartable_agent;

use br_runner_jobs::test_double::ScriptedJobs;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use support::{call_with, has_terminal, push_mode, settings, trigger, until};

#[tokio::test(start_paused = true)]
async fn the_restartable_agent_example_answers_saves_the_state_and_calls_the_hook() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(support::writes_ok());
    jobs.push_trigger(call_with(trigger(1), &[("mode", json!("SYNC"))]));
    let shutdown = CancellationToken::new();
    let closed = restartable_agent::Closed::default();
    let done = tokio::spawn(restartable_agent::serve(
        jobs.client(),
        settings(1),
        shutdown.clone(),
        closed.clone(),
    ));
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    done.await.expect("joined").expect("served");
    assert_eq!(
        closed.lock().expect("lock").clone(),
        vec!["greeter/assistant".to_owned()]
    );
    let uploads = jobs
        .calls()
        .into_iter()
        .filter(|call| matches!(call, br_runner_jobs::test_double::Call::Graphql { document, .. } if document.contains("jobsUploadResult") || document.contains("jobsSaveState")))
        .count();
    assert_eq!(uploads, 2);
}
