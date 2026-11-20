mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use br_runner_jobs::test_double::{Delivery, ScriptedJobs};
use br_runner_jobs::{CancelEvent, DispatchMode, FailureKind, ModeEvent};
use br_runner_toolkit::{
    FailureOverride, RunRefusal, StartArgs, StartedRun, failure_mapping, run_start,
};
use serde_json::json;
use support::*;
use tokio::sync::Notify;

fn marking_args(started: &Arc<AtomicBool>) -> StartArgs<Flow> {
    let flag = Arc::clone(started);
    StartArgs::new(
        single(Behavior::Result("x")),
        run_start(move |_input| {
            flag.store(true, Ordering::SeqCst);
            async { Ok::<_, RunRefusal>(StartedRun::new(Flow::default(), vec!["work".to_owned()])) }
        }),
        false,
    )
}

fn cancel_known_when_taken(jobs: &ScriptedJobs, mode: DispatchMode) {
    jobs.push_mode(Ok(ModeEvent::Mode(mode)));
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(101) }));
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
}

#[derive(Debug, PartialEq)]
struct Seen {
    reason_code: String,
    text: String,
    has_cause: bool,
}

fn recording_mapping(seen: &Arc<Mutex<Vec<Seen>>>) -> Arc<dyn br_runner_toolkit::FailureMapping> {
    let seen = Arc::clone(seen);
    failure_mapping(move |failure| {
        seen.lock().expect("lock").push(Seen {
            reason_code: failure.reason_code.to_owned(),
            text: failure.text.to_owned(),
            has_cause: failure.cause.is_some(),
        });
        FailureOverride {
            kind: Some(FailureKind::Transient),
            diagnostic: Some(json!("stopped by the requester")),
            ..FailureOverride::default()
        }
    })
}

fn assert_mapped_once(jobs: &ScriptedJobs, seen: &Mutex<Vec<Seen>>) {
    assert_eq!(
        *seen.lock().expect("lock"),
        vec![Seen {
            reason_code: "run_cancelled".to_owned(),
            text: String::new(),
            has_cause: false,
        }]
    );
    assert_eq!(kinds(jobs), vec!["started", "failed"]);
    let failed = failed_fact(jobs).expect("a failed fact");
    assert_eq!(failed.reason_code, "run_cancelled");
    assert_eq!(failed.kind, FailureKind::Transient);
    assert_eq!(failed.diagnostic, json!("stopped by the requester"));
    assert_eq!(failed.params, json!({}));
}

#[tokio::test(start_paused = true)]
async fn given_a_cancel_order_for_a_run_in_flight_when_it_comes_then_the_run_ends_with_the_failed_fact_run_cancelled_and_the_slot_is_free()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::UntilCancelled), false),
    );
    until(|| kinds(&jobs) == vec!["started"]).await;
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(101) }));
    until(|| has_terminal(&jobs)).await;
    assert_run_cancelled(&jobs);
    jobs.push_trigger(trigger(2));
    until(|| kinds(&jobs).len() == 3).await;
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_cancel_order_known_when_the_run_is_taken_in_push_mode_then_the_graph_is_not_started_and_the_run_ends_with_the_failed_fact_run_cancelled()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    cancel_known_when_taken(&jobs, DispatchMode::Push);
    jobs.push_trigger(trigger(1));
    let started = Arc::new(AtomicBool::new(false));
    let running = run(&jobs, |b| b, 1, marking_args(&started));
    until(|| has_terminal(&jobs)).await;
    assert_run_cancelled(&jobs);
    assert_eq!(jobs.deliveries(), vec![Delivery::Ack]);
    assert!(!started.load(Ordering::SeqCst));
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_cancel_order_known_when_the_run_is_claimed_then_the_graph_is_not_started_and_the_run_ends_with_the_failed_fact_run_cancelled()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    cancel_known_when_taken(&jobs, DispatchMode::Claim);
    jobs.push_claim_reply(Ok(served(1)));
    jobs.push_claim_reply(Ok(refusal("not_claim_mode")));
    let started = Arc::new(AtomicBool::new(false));
    let running = run(&jobs, |b| b, 1, marking_args(&started));
    until(|| has_terminal(&jobs)).await;
    assert_run_cancelled(&jobs);
    assert!(jobs.deliveries().is_empty());
    assert!(!started.load(Ordering::SeqCst));
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_cancel_order_seen_while_the_run_start_function_runs_when_it_returns_then_the_executor_does_not_start_and_the_run_ends_with_the_failed_fact_run_cancelled()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (signal, wait) = (Arc::clone(&entered), Arc::clone(&release));
    let held = StartArgs::new(
        single(Behavior::Result("x")),
        run_start(move |_input| {
            let (signal, wait) = (Arc::clone(&signal), Arc::clone(&wait));
            async move {
                signal.notify_one();
                wait.notified().await;
                Ok::<_, RunRefusal>(StartedRun::new(Flow::default(), vec!["work".to_owned()]))
            }
        }),
        false,
    );
    let running = run(&jobs, |b| b, 1, held);
    entered.notified().await;
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(101) }));
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    release.notify_one();
    until(|| has_terminal(&jobs)).await;
    assert_run_cancelled(&jobs);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_failure_mapping_when_a_run_in_flight_is_cancelled_then_the_mapping_gets_run_cancelled_with_an_empty_text_and_its_override_reaches_jobs()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let seen = Arc::default();
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::UntilCancelled), false)
            .with_failure_mapping(recording_mapping(&seen)),
    );
    until(|| kinds(&jobs) == vec!["started"]).await;
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(101) }));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_mapped_once(&jobs, &seen);
}

#[tokio::test(start_paused = true)]
async fn given_a_failure_mapping_when_a_cancel_order_ends_the_run_before_the_executor_starts_then_the_mapping_gets_run_cancelled_and_its_override_reaches_jobs()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    cancel_known_when_taken(&jobs, DispatchMode::Push);
    jobs.push_trigger(trigger(1));
    let seen = Arc::default();
    let started = Arc::new(AtomicBool::new(false));
    let running = run(
        &jobs,
        |b| b,
        1,
        marking_args(&started).with_failure_mapping(recording_mapping(&seen)),
    );
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
    assert_mapped_once(&jobs, &seen);
    assert!(!started.load(Ordering::SeqCst));
}

#[tokio::test(start_paused = true)]
async fn given_a_cancel_order_for_another_run_when_it_comes_then_the_run_goes_on() {
    let gate = Arc::new(Notify::new());
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    until(|| kinds(&jobs) == vec!["started"]).await;
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(999) }));
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    gate.notify_one();
    until(|| has_terminal(&jobs)).await;
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
    running.stop().await;
}
