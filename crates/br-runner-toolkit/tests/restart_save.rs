mod support;

use std::sync::Arc;

use br_runner_jobs::test_double::{Call, Fact, ScriptedJobs};
use br_runner_toolkit::RunnerBuilder;
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn inbox_agent(builder: &RunnerBuilder, inbox: bool) -> Agent {
    Agent {
        lookup: builder.instance_lookup(),
        label: "main",
        inbox,
        seen_run: Default::default(),
    }
}

async fn run_with_agent(can_restart: bool, inbox: bool) -> Arc<ScriptedJobs> {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
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
    assert_eq!(state["agents"].as_array().expect("agents").len(), 1);
    let registered = registrations(&jobs);
    assert_eq!(registered.len(), 1);
    assert_eq!(state["agents"][0]["agent_id"], json!(registered[0].0));
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
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
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

fn registrations(jobs: &ScriptedJobs) -> Vec<(Uuid, bool, bool)> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Register { agents, .. } => Some(agents),
            _ => None,
        })
        .flatten()
        .map(|agent| (agent.agent_id, agent.has_inbox, agent.can_restart))
        .collect()
}

#[tokio::test(start_paused = true)]
async fn given_a_restart_whose_saved_state_holds_an_agent_with_an_inbox_when_the_node_asks_for_its_agent_then_the_registration_carries_the_saved_agent_id_and_inbox_flag()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    let state = json!({
        "fields": { "steps": ["old"] },
        "agents": [{
            "node": "work",
            "label": "main",
            "agent_id": id(900),
            "has_inbox": true
        }]
    });
    let mut reply = saved_state_reply(state);
    reply
        .as_object_mut()
        .expect("object")
        .extend(writes_ok().as_object().expect("object").clone());
    jobs.graphql_default(reply);
    jobs.push_trigger(restart_trigger(1));
    let builder = RunnerBuilder::new(jobs.client(), settings(1));
    let agent = inbox_agent(&builder, true);
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(
        args(agent_graph(Behavior::Result("x"), agent), true).with_shutdown(shutdown.clone()),
    ));
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    done.await.expect("joined").expect("started");
    assert_eq!(registrations(&jobs), vec![(id(900), true, true)]);
}
