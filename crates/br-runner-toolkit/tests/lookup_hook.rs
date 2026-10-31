mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_graph_contract::InstanceId;
use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_toolkit::{
    ErrorCode, InstanceEnded, RunnerBuilder, lifecycle_hook,
};
use support::*;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

fn unknown_instance() -> InstanceId {
    InstanceId::new(id(777)).expect("a UUIDv7")
}

#[tokio::test(start_paused = true)]
async fn given_a_lookup_when_called_before_the_runner_started_then_it_is_an_error_of_the_lookup() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    let builder = RunnerBuilder::new(jobs.client(), settings(1));
    let lookup = builder.instance_lookup();
    let error = lookup.book(unknown_instance()).await.expect_err("an error");
    assert_eq!(error.code(), ErrorCode::NotStarted);
    let error = lookup
        .run_context(unknown_instance())
        .await
        .expect_err("an error");
    assert_eq!(error.code(), ErrorCode::NotStarted);
}

#[tokio::test(start_paused = true)]
async fn given_an_instance_the_library_does_not_hold_when_looked_up_then_the_lookup_waits() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    let builder = RunnerBuilder::new(jobs.client(), settings(1));
    let lookup = builder.instance_lookup();
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(args(single(Behavior::Result("x")), false).with_shutdown(shutdown.clone())));
    tokio::time::sleep(Duration::from_secs(1)).await;
    let waited = tokio::time::timeout(Duration::from_secs(30), lookup.book(unknown_instance())).await;
    assert!(waited.is_err());
    shutdown.cancel();
    done.await.expect("joined").expect("started");
}

#[tokio::test(start_paused = true)]
async fn given_a_node_that_asks_the_lookup_when_it_runs_then_it_gets_the_agent_book_and_the_run_context_of_its_run() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let builder = RunnerBuilder::new(jobs.client(), settings(1));
    let agent = Agent {
        lookup: builder.instance_lookup(),
        label: "main",
        inbox: true,
    };
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(
        args(agent_graph(Behavior::Result("x"), agent), false).with_shutdown(shutdown.clone()),
    ));
    until(|| has_terminal(&jobs)).await;
    let registered = jobs
        .calls()
        .into_iter()
        .any(|call| matches!(call, br_runner_jobs::test_double::Call::Register { .. }));
    assert!(registered);
    shutdown.cancel();
    done.await.expect("joined").expect("started");
}

#[tokio::test(start_paused = true)]
async fn given_a_lifecycle_hook_when_an_instance_ends_then_the_hook_is_called_once_with_the_node_and_its_agents() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let calls: Arc<Mutex<Vec<InstanceEnded>>> = Arc::default();
    let seen = Arc::clone(&calls);
    let hook = lifecycle_hook(move |ended| {
        let seen = Arc::clone(&seen);
        async move {
            seen.lock().expect("lock").push(ended);
        }
    });
    let builder = RunnerBuilder::new(jobs.client(), settings(1)).with_lifecycle_hook(hook);
    let agent = Agent {
        lookup: builder.instance_lookup(),
        label: "main",
        inbox: true,
    };
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(
        args(agent_graph(Behavior::Result("x"), agent), false).with_shutdown(shutdown.clone()),
    ));
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    done.await.expect("joined").expect("started");
    let calls = calls.lock().expect("lock");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].node, "work");
    assert_eq!(calls[0].agents.len(), 1);
    assert_eq!(calls[0].agents[0].key.label, "main");
    assert!(calls[0].agents[0].has_inbox);
}

#[tokio::test(start_paused = true)]
async fn given_a_hook_that_has_not_returned_when_the_executor_has_returned_then_the_run_is_not_settled_until_the_hook_returns() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    jobs.push_trigger(trigger(2));
    let release = Arc::new(Notify::new());
    let gate = Arc::clone(&release);
    let hook = lifecycle_hook(move |_ended| {
        let gate = Arc::clone(&gate);
        async move {
            gate.notified().await;
        }
    });
    let builder = RunnerBuilder::new(jobs.client(), settings(1)).with_lifecycle_hook(hook);
    let agent = Agent {
        lookup: builder.instance_lookup(),
        label: "main",
        inbox: false,
    };
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(
        args(agent_graph(Behavior::Result("x"), agent), false).with_shutdown(shutdown.clone()),
    ));
    until(|| kinds(&jobs).contains(&"started")).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(kinds(&jobs), vec!["started"]);
    release.notify_one();
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    done.await.expect("joined").expect("started");
}

#[tokio::test(start_paused = true)]
async fn given_a_hook_that_panics_when_an_instance_ends_then_the_panic_is_caught_and_the_run_settles() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let hook = lifecycle_hook(|_ended| async {
        if true {
            panic!("the hook panics");
        }
    });
    let builder = RunnerBuilder::new(jobs.client(), settings(1)).with_lifecycle_hook(hook);
    let agent = Agent {
        lookup: builder.instance_lookup(),
        label: "main",
        inbox: false,
    };
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(
        args(agent_graph(Behavior::Result("x"), agent), false).with_shutdown(shutdown.clone()),
    ));
    until(|| has_terminal(&jobs)).await;
    shutdown.cancel();
    assert!(done.await.expect("joined").expect("started").runs.is_empty());
}
