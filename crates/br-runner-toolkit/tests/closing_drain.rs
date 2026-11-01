mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_graph_contract::InstanceId;
use br_runner_jobs::WriteKind;
use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_toolkit::{InstanceEnded, RunnerBuilder, lifecycle_hook};
use support::*;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

async fn run_with_a_held_terminal_fact() -> (Arc<ScriptedJobs>, Running, Arc<Notify>) {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let gate = Arc::new(Notify::new());
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    until(|| kinds(&jobs) == vec!["started"]).await;
    jobs.pause();
    gate.notify_one();
    (jobs, running, gate)
}

#[tokio::test(start_paused = true)]
async fn given_a_run_whose_terminal_fact_is_held_when_the_graph_ends_then_the_slot_is_free_and_takes_the_next_trigger_while_the_closing_waits()
 {
    let (jobs, running, _gate) = run_with_a_held_terminal_fact().await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    jobs.push_trigger(trigger(2));
    until(|| jobs.triggers_waiting() == 0).await;
    assert!(!has_terminal(&jobs));
    jobs.resume();
    until(|| kinds(&jobs).contains(&"completed")).await;
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_closing_that_waits_when_the_shutdown_comes_then_the_drain_waits_for_it_and_at_the_timeout_it_drops_the_queue_and_the_report_lists_the_unsent_write()
 {
    let (logs, _guard) = Logs::capture();
    let (jobs, running, _gate) = run_with_a_held_terminal_fact().await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    running.shutdown.cancel();
    tokio::time::sleep(Duration::from_secs(50)).await;
    assert!(!running.done.is_finished());
    tokio::time::sleep(Duration::from_secs(20)).await;
    let report = running.done.await.expect("joined").expect("started");
    assert_eq!(report.runs.len(), 1);
    let drained = &report.runs[0];
    assert!(!drained.aborted);
    assert_eq!(drained.run_id, id(101));
    assert_eq!(drained.job_id, id(201));
    assert!(drained.instances_without_return.is_empty());
    let unsent: Vec<WriteKind> = drained.unsent.iter().map(|write| write.kind).collect();
    assert_eq!(unsent, vec![WriteKind::Completed]);
    assert_eq!(
        logs.containing(tracing::Level::ERROR, "a write of the run was removed"),
        unsent.len()
    );
    assert_eq!(jobs.presence_deletions(), 1);
    jobs.resume();
}

#[tokio::test(start_paused = true)]
async fn given_a_run_in_flight_with_a_hook_when_the_drain_time_ends_then_the_hook_is_called_with_the_agents_before_the_book_is_dropped_and_the_report_lists_the_instance()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let calls: Arc<Mutex<Vec<InstanceEnded>>> = Arc::default();
    let seen = Arc::clone(&calls);
    let hook = lifecycle_hook(move |ended| {
        let seen = Arc::clone(&seen);
        async move {
            seen.lock().expect("lock").push(ended);
            std::future::pending::<()>().await;
        }
    });
    let builder = RunnerBuilder::new(jobs.client(), settings(1)).with_lifecycle_hook(hook);
    let agent = Agent {
        lookup: builder.instance_lookup(),
        label: "main",
        inbox: true,
        seen_run: Default::default(),
    };
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(
        args(agent_graph(Behavior::UntilCancelled, agent), false).with_shutdown(shutdown.clone()),
    ));
    until(|| kinds(&jobs) == vec!["started"]).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    shutdown.cancel();
    tokio::time::sleep(Duration::from_secs(61)).await;
    let report = done.await.expect("joined").expect("started");
    let calls = calls.lock().expect("lock");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].agents.len(), 1);
    assert_eq!(calls[0].agents[0].key.label, "main");
    assert_eq!(report.runs.len(), 1);
    assert!(report.runs[0].aborted);
    let instances: Vec<InstanceId> = vec![calls[0].instance_id];
    assert_eq!(report.runs[0].instances_without_return, instances);
}

#[tokio::test(start_paused = true)]
async fn given_a_hook_that_was_called_and_never_returns_when_the_drain_time_ends_then_the_report_lists_its_instance()
 {
    let (logs, _guard) = Logs::capture();
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let called: Arc<Mutex<Vec<InstanceId>>> = Arc::default();
    let seen = Arc::clone(&called);
    let hook = lifecycle_hook(move |ended| {
        let seen = Arc::clone(&seen);
        async move {
            seen.lock().expect("lock").push(ended.instance_id);
            std::future::pending::<()>().await;
        }
    });
    let builder = RunnerBuilder::new(jobs.client(), settings(1)).with_lifecycle_hook(hook);
    let agent = Agent {
        lookup: builder.instance_lookup(),
        label: "main",
        inbox: false,
        seen_run: Default::default(),
    };
    let shutdown = CancellationToken::new();
    let done = tokio::spawn(builder.start(
        args(agent_graph(Behavior::Result("x"), agent), false).with_shutdown(shutdown.clone()),
    ));
    until(|| !called.lock().expect("lock").is_empty()).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    shutdown.cancel();
    tokio::time::sleep(Duration::from_secs(61)).await;
    let report = done.await.expect("joined").expect("started");
    assert_eq!(report.runs.len(), 1);
    assert!(report.runs[0].aborted);
    assert_eq!(
        report.runs[0].instances_without_return,
        called.lock().expect("lock").clone()
    );
    assert_eq!(
        logs.containing(tracing::Level::ERROR, "the drain aborts a run"),
        1
    );
    assert_eq!(logs.containing(tracing::Level::ERROR, "writes_left"), 0);
    assert!(!has_terminal(&jobs));
}

#[tokio::test(start_paused = true)]
async fn given_no_hook_when_the_drain_time_ends_then_the_report_lists_no_instance() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::UntilCancelled), false),
    );
    until(|| kinds(&jobs) == vec!["started"]).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    running.shutdown.cancel();
    tokio::time::sleep(Duration::from_secs(61)).await;
    let report = running.done.await.expect("joined").expect("started");
    assert_eq!(report.runs.len(), 1);
    assert!(report.runs[0].aborted);
    assert!(report.runs[0].instances_without_return.is_empty());
}
