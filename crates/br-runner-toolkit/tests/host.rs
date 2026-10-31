mod support;

use std::time::Duration;

use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_jobs::{CancelEvent, PresenceStatus};
use br_runner_toolkit::{HostEvent, Settings, TestHost, advance_clock};
use support::*;

fn host_settings() -> br_runner_toolkit::SettingsBuilder {
    Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(8)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(1))
        .cancel_grace(Duration::from_secs(1))
        .claim_wait(Duration::from_secs(5))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1))
}

#[tokio::test(start_paused = true)]
async fn given_a_test_host_when_one_run_ends_then_the_host_returns_its_events_in_order() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let host = TestHost::new(
        jobs.client(),
        host_settings(),
        args(single(Behavior::Result("x")), false),
    )
    .expect("a host");
    let outcome = host.run().await.expect("a run");
    let position = |wanted: &dyn Fn(&HostEvent) -> bool| {
        outcome.events.iter().position(wanted).expect("an event")
    };
    let published = position(&|event| {
        matches!(
            event,
            HostEvent::PresencePublished { status: PresenceStatus::Ready, .. }
        )
    });
    let acknowledged = position(&|event| matches!(event, HostEvent::TriggerAcknowledged));
    let deleted = position(&|event| matches!(event, HostEvent::PresenceDeleted));
    assert!(published < acknowledged);
    assert!(acknowledged < deleted);
    assert!(outcome.drain.runs.is_empty());
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
}

#[tokio::test(start_paused = true)]
async fn given_a_test_host_when_it_runs_then_it_takes_one_run_even_if_more_triggers_wait() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    jobs.push_trigger(trigger(2));
    let host = TestHost::new(
        jobs.client(),
        host_settings(),
        args(single(Behavior::Result("x")), false),
    )
    .expect("a host");
    host.run().await.expect("a run");
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
}

#[tokio::test(start_paused = true)]
async fn given_a_test_host_and_a_run_that_waits_when_the_cancel_handle_is_used_then_the_run_ends_without_a_terminal_fact() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let host = TestHost::new(
        jobs.client(),
        host_settings(),
        args(single(Behavior::UntilCancelled), false),
    )
    .expect("a host");
    let cancel = host.cancel_handle();
    let handle = tokio::spawn(host.run());
    until(|| kinds(&jobs) == vec!["started"]).await;
    cancel.cancel();
    let outcome = handle.await.expect("joined").expect("a run");
    assert!(!has_terminal(&jobs));
    assert!(outcome.drain.runs.is_empty());
}

#[tokio::test(start_paused = true)]
async fn given_a_test_host_when_a_cancel_order_is_made_then_the_run_ends_and_the_clock_can_be_advanced() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let host = TestHost::new(
        jobs.client(),
        host_settings(),
        args(single(Behavior::UntilCancelled), false),
    )
    .expect("a host");
    let handle = tokio::spawn(host.run());
    until(|| kinds(&jobs) == vec!["started"]).await;
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(101) }));
    advance_clock(Duration::from_secs(5)).await;
    handle.await.expect("joined").expect("a run");
    assert!(!has_terminal(&jobs));
}

#[tokio::test(start_paused = true)]
async fn given_invalid_settings_when_a_test_host_is_made_then_it_is_an_error() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    let error = TestHost::new(
        jobs.client(),
        Settings::builder(),
        args(single(Behavior::Result("x")), false),
    )
    .err()
    .expect("an error");
    assert_eq!(error.code(), br_runner_toolkit::ErrorCode::SettingMissing);
}
