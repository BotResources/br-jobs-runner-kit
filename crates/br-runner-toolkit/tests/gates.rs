mod support;

use std::time::Duration;

use br_runner_jobs::test_double::{Call, ScriptedJobs};
use br_runner_jobs::{CancelEvent, DispatchMode, ModeEvent};
use serde_json::json;
use support::*;

fn claims(jobs: &ScriptedJobs) -> usize {
    jobs.calls()
        .into_iter()
        .filter(|call| matches!(call, Call::Claim { .. }))
        .count()
}

#[tokio::test(start_paused = true)]
async fn given_no_dispatch_mode_when_a_trigger_is_made_then_the_runner_takes_nothing() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert!(kinds(&jobs).is_empty());
    assert_eq!(claims(&jobs), 0);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_no_entry_event_when_a_trigger_is_made_then_the_runner_takes_nothing() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    jobs.push_mode(Ok(ModeEvent::NoEntry));
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert!(kinds(&jobs).is_empty());
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_cancel_replay_that_is_not_complete_when_a_trigger_is_made_then_the_runner_takes_nothing_until_it_is_complete() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Push)));
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert!(kinds(&jobs).is_empty());
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_claim_mode_and_a_cancel_replay_that_is_not_complete_then_no_claim_is_sent() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Claim)));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert_eq!(claims(&jobs), 0);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_mode_that_goes_from_push_to_no_entry_and_back_when_a_trigger_waits_then_it_is_taken_after_the_return() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_mode(Ok(ModeEvent::NoEntry));
    tokio::time::sleep(Duration::from_secs(1)).await;
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(5)).await;
    jobs.push_trigger(trigger(1));
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(kinds(&jobs).is_empty());
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Push)));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_mode_that_goes_from_push_to_claim_when_the_runner_is_idle_then_it_sends_claims() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(claims(&jobs), 0);
    jobs.push_claim_reply(Ok(serde_json::to_vec(
        &json!({ "version": 2, "reason": "not_in_claim_mode" }),
    )
    .expect("bytes")));
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Claim)));
    until(|| claims(&jobs) >= 1).await;
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_two_slots_when_two_triggers_are_made_then_two_runs_are_in_flight_at_once() {
    let gate = std::sync::Arc::new(tokio::sync::Notify::new());
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    jobs.push_trigger(trigger(2));
    jobs.push_trigger(trigger(3));
    let running = run(
        &jobs,
        |b| b,
        2,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    until(|| kinds(&jobs).len() == 2).await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert_eq!(kinds(&jobs), vec!["started", "started"]);
    gate.notify_waiters();
    until(|| kinds(&jobs).len() >= 4).await;
    gate.notify_waiters();
    running.stop().await;
}
