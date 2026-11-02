mod support;

#[path = "../examples/restartable_agent.rs"]
#[allow(dead_code)] // the example holds code that only its own main uses
mod restartable_agent;

use br_runner_jobs::test_double::ScriptedJobs;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use support::{call_with, has_terminal, push_mode, runner_type, settings, trigger, until};

#[tokio::test(start_paused = true)]
async fn the_restartable_agent_example_answers_saves_the_state_and_calls_the_hook() {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(support::writes_ok());
    jobs.push_trigger(call_with(trigger(1), &[("mode", json!("SYNC"))]));
    let shutdown = CancellationToken::new();
    let observed = restartable_agent::Observed::default();
    let done = tokio::spawn(restartable_agent::serve(
        jobs.client(),
        settings(1),
        shutdown.clone(),
        observed.clone(),
    ));
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    done.await.expect("joined").expect("served");
    assert_eq!(
        observed.closed.lock().expect("lock").clone(),
        vec!["greeter/assistant".to_owned()]
    );
    let stamps = observed.stamps.lock().expect("lock").clone();
    assert_eq!(stamps.len(), 1);
    assert_eq!(stamps[0].job_id, support::id(201));
    let uploads = jobs
        .calls()
        .into_iter()
        .filter(|call| matches!(call, br_runner_jobs::test_double::Call::Graphql { document, .. } if document.contains("jobsUploadResult") || document.contains("jobsSaveState")))
        .count();
    assert_eq!(uploads, 2);
}

#[tokio::test]
async fn the_stored_gate_of_the_example_goes_through_the_stand_in_entry_stored() {
    let observed = restartable_agent::Observed::default();
    let gate = restartable_agent::stored_gate(restartable_agent::write_context(&observed));
    let entry = br_llm_messages::EntryId::from_uuid(support::id(7)).expect("an entry id");
    let thread = br_agentic_contract::ThreadRef::from_uuid(support::id(8)).expect("a thread ref");
    assert_eq!(gate(thread, entry).await, br_runner_jobs::Gate::Stored);
    assert_eq!(
        observed.entries.lock().expect("lock").clone(),
        vec![support::id(7)]
    );
}
