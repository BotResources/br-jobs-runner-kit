mod support;

use std::sync::{Arc, Mutex};

use br_runner_jobs::test_double::{Call, ScriptedJobs};
use br_runner_jobs::{FailureKind, test_double::Fact};
use br_runner_toolkit::{RunRefusal, RunnerBuilder, StartArgs, StartedRun, run_start};
use serde_json::{Value, json};
use support::*;
use tokio_util::sync::CancellationToken;

fn restart_trigger(n: u128) -> Vec<u8> {
    call_with(
        trigger(n),
        &[("restart", json!({ "restart_of_run_id": id(50) }))],
    )
}

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

fn writes(jobs: &ScriptedJobs) -> Vec<Value> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Graphql {
                document,
                variables,
            } if document.contains("jobsSaveState") => Some(variables),
            _ => None,
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn given_a_restart_when_the_run_starts_then_the_saved_state_is_read_once_and_given_to_the_run_start_function()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
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
    let jobs = ScriptedJobs::new("analyst", "i1");
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
    let jobs = ScriptedJobs::new("analyst", "i1");
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
    let jobs = ScriptedJobs::new("analyst", "i1");
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
    let jobs = ScriptedJobs::new("analyst", "i1");
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
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(restart_trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), true));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    let failed = failed_fact(&jobs).expect("failed");
    assert_eq!(failed.reason_code, "saved_state_unavailable");
    assert_eq!(failed.kind, FailureKind::Transient);
}

fn inbox_agent(builder: &RunnerBuilder, inbox: bool) -> Agent {
    Agent {
        lookup: builder.instance_lookup(),
        label: "main",
        inbox,
        seen_run: Default::default(),
    }
}

async fn run_with_agent(can_restart: bool, inbox: bool) -> Arc<ScriptedJobs> {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(writes_ok());
    jobs.push_trigger(trigger(1));
    let builder = RunnerBuilder::new(jobs.client(), settings(1));
    let agent = inbox_agent(&builder, inbox);
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(
        builder.start(
            args(agent_graph(Behavior::Result("x"), agent), can_restart)
                .with_shutdown(shutdown.clone()),
        ),
    );
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    done.await.expect("joined").expect("started");
    jobs
}

#[tokio::test(start_paused = true)]
async fn given_a_restartable_runner_and_an_agent_with_an_inbox_when_the_graph_ends_then_the_state_and_the_agents_are_saved()
 {
    let jobs = run_with_agent(true, true).await;
    let saves = writes(&jobs);
    assert_eq!(saves.len(), 1);
    let state = &saves[0]["input"]["state"];
    assert_eq!(state["fields"], json!({ "steps": ["x"] }));
    assert_eq!(state["agents"][0]["node"], json!("work"));
    assert_eq!(state["agents"][0]["label"], json!("main"));
    assert_eq!(state["agents"][0]["has_inbox"], json!(true));
}

#[tokio::test(start_paused = true)]
async fn given_a_runner_that_cannot_restart_when_the_graph_ends_then_no_state_is_saved() {
    let jobs = run_with_agent(false, true).await;
    assert!(writes(&jobs).is_empty());
}

#[tokio::test(start_paused = true)]
async fn given_agents_without_an_inbox_when_the_graph_ends_then_no_state_is_saved() {
    let jobs = run_with_agent(true, false).await;
    assert!(writes(&jobs).is_empty());
}

#[tokio::test(start_paused = true)]
async fn given_a_refused_state_save_when_the_run_completes_then_the_run_fails_with_saved_state_refused()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.graphql_default(json!({
        "jobsSaveState": { "success": false, "reason": "too_big", "message": "too big" }
    }));
    jobs.push_trigger(trigger(1));
    let builder = RunnerBuilder::new(jobs.client(), settings(1));
    let agent = inbox_agent(&builder, true);
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(
        args(agent_graph(Behavior::Result("x"), agent), true).with_shutdown(shutdown.clone()),
    ));
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    done.await.expect("joined").expect("started");
    assert_eq!(
        failed_fact(&jobs).expect("failed").reason_code,
        "saved_state_refused"
    );
    assert!(
        !facts(&jobs)
            .iter()
            .any(|fact| matches!(fact, Fact::Completed { .. }))
    );
}
