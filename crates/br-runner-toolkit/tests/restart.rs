mod support;

use std::sync::{Arc, Mutex};

use br_runner_jobs::FailureKind;
use br_runner_jobs::test_double::{Call, ScriptedJobs};
use br_runner_toolkit::{RunRefusal, StartArgs, StartedRun, run_start};
use serde_json::{Value, json};
use support::*;

fn recording_args(
    graph: Arc<br_graph_contract::Graph<Flow>>,
    can_restart: bool,
    seen: Arc<Mutex<Vec<Option<Flow>>>>,
) -> StartArgs<Flow> {
    StartArgs::new(
        graph,
        run_start(move |input| {
            let seen = Arc::clone(&seen);
            async move {
                seen.lock().expect("lock").push(input.restored.clone());
                Ok::<_, RunRefusal>(StartedRun::new(
                    input.restored.unwrap_or_default(),
                    vec!["unused".to_owned()],
                ))
            }
        }),
        can_restart,
    )
}

fn saved(fields: Value) -> Value {
    json!({ "fields": fields, "agents": [] })
}

fn reads(jobs: &ScriptedJobs) -> usize {
    jobs.calls()
        .into_iter()
        .filter(|call| matches!(call, Call::Graphql { document, .. } if document.contains("jobsSavedState")))
        .count()
}

#[tokio::test(start_paused = true)]
async fn given_a_restart_when_the_run_starts_then_the_saved_state_is_read_once_and_given_to_the_run_start_function()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    let mut reply = saved_state_reply(saved(json!({ "steps": ["old"] })));
    reply
        .as_object_mut()
        .expect("object")
        .extend(writes_ok().as_object().expect("object").clone());
    jobs.graphql_default(reply);
    jobs.push_trigger(restart_trigger(1));
    let seen = Arc::default();
    let running = run(
        &jobs,
        |b| b,
        1,
        recording_args(single(Behavior::Result("new")), true, Arc::clone(&seen)),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_eq!(reads(&jobs), 1);
    assert_eq!(
        seen.lock().expect("lock").clone(),
        vec![Some(Flow {
            steps: vec!["old".to_owned()]
        })]
    );
}

#[tokio::test(start_paused = true)]
async fn given_a_restart_when_the_graph_starts_then_it_starts_at_the_entry_nodes_of_the_graph() {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    let mut reply = saved_state_reply(saved(json!({ "steps": ["old"] })));
    reply
        .as_object_mut()
        .expect("object")
        .extend(writes_ok().as_object().expect("object").clone());
    jobs.graphql_default(reply);
    jobs.push_trigger(restart_trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        recording_args(single(Behavior::Result("new")), true, Arc::default()),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
}

#[tokio::test(start_paused = true)]
async fn given_a_run_that_is_not_a_restart_when_it_starts_then_the_saved_state_is_not_read() {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(writes_ok());
    jobs.push_trigger(trigger(1));
    let seen = Arc::default();
    let running = run(
        &jobs,
        |b| b,
        1,
        recording_args(single(Behavior::Result("new")), true, Arc::clone(&seen)),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_eq!(reads(&jobs), 0);
    assert_eq!(seen.lock().expect("lock").clone(), vec![None]);
}

#[tokio::test(start_paused = true)]
async fn given_a_saved_state_that_jobs_refuses_when_a_restart_starts_then_the_run_fails_permanently_with_saved_state_invalid()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(json!({
        "jobsSavedState": { "success": false, "reason": "no_state", "message": "nothing saved" }
    }));
    jobs.push_trigger(restart_trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), true));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let failed = failed_fact(&jobs).expect("failed");
    assert_eq!(failed.reason_code, "saved_state_invalid");
    assert_eq!(failed.kind, FailureKind::Permanent);
}

#[tokio::test(start_paused = true)]
async fn given_a_saved_state_that_does_not_fit_the_fields_when_a_restart_starts_then_the_run_fails_permanently_with_saved_state_invalid()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(saved_state_reply(json!("garbage")));
    jobs.push_trigger(restart_trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), true));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_eq!(
        failed_fact(&jobs).expect("failed").reason_code,
        "saved_state_invalid"
    );
}

#[tokio::test(start_paused = true)]
async fn given_a_saved_state_that_cannot_be_reached_when_a_restart_starts_then_the_run_fails_as_transient_with_saved_state_unavailable()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(restart_trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), true));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let failed = failed_fact(&jobs).expect("failed");
    assert_eq!(failed.reason_code, "saved_state_unavailable");
    assert_eq!(failed.kind, FailureKind::Transient);
}
