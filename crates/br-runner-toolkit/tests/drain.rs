mod support;

use std::sync::Arc;
use std::time::Duration;

use br_runner_jobs::PresenceStatus;
use br_runner_jobs::test_double::ScriptedJobs;
use support::*;
use tokio::sync::Notify;

#[tokio::test(start_paused = true)]
async fn given_a_drain_with_no_run_in_flight_when_the_shutdown_comes_then_the_report_is_empty_and_the_presence_is_deleted()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(1)).await;
    let report = running.stop().await;
    assert!(report.runs.is_empty());
    assert_eq!(jobs.presence_deletions(), 1);
}

#[tokio::test(start_paused = true)]
async fn given_a_drain_when_the_shutdown_comes_then_the_presence_goes_to_draining() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    let gate = Arc::new(Notify::new());
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    until(|| kinds(&jobs) == vec!["started"]).await;
    running.shutdown.cancel();
    tokio::time::sleep(Duration::from_secs(20)).await;
    let records = jobs.presence_records();
    assert_eq!(
        records.last().map(|record| record.status),
        Some(PresenceStatus::Draining)
    );
    gate.notify_waiters();
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_run_in_flight_when_the_shutdown_comes_then_the_drain_waits_for_the_run_to_end() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    let gate = Arc::new(Notify::new());
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    until(|| kinds(&jobs) == vec!["started"]).await;
    running.shutdown.cancel();
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(!running.done.is_finished());
    gate.notify_waiters();
    let report = running.done.await.expect("joined").expect("started");
    assert!(report.runs.is_empty());
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
    assert_eq!(jobs.presence_deletions(), 1);
}

#[tokio::test(start_paused = true)]
async fn given_a_run_in_flight_when_the_drain_time_ends_then_the_run_is_aborted_and_the_report_names_it()
 {
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
    running.shutdown.cancel();
    tokio::time::sleep(Duration::from_secs(61)).await;
    let report = running.done.await.expect("joined").expect("started");
    assert_eq!(report.runs.len(), 1);
    assert!(report.runs[0].aborted);
    assert!(report.runs[0].executor_running);
    assert_eq!(report.runs[0].run_id, id(101));
    assert_eq!(report.runs[0].job_id, id(201));
    assert_eq!(jobs.presence_deletions(), 1);
    assert!(!has_terminal(&jobs));
}

#[tokio::test(start_paused = true)]
async fn given_a_drain_when_a_trigger_is_made_after_it_began_then_the_trigger_is_not_taken() {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(1)).await;
    running.shutdown.cancel();
    let report = running.done.await.expect("joined").expect("started");
    jobs.push_trigger(trigger(1));
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert!(report.runs.is_empty());
    assert!(kinds(&jobs).is_empty());
    assert!(jobs.deliveries().is_empty());
}

#[tokio::test(start_paused = true)]
async fn given_a_run_in_flight_when_the_drain_time_ends_then_the_cancel_token_of_the_run_is_triggered_for_the_work_the_node_started_by_itself()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let noted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::NotingCancel(Arc::clone(&noted))), false),
    );
    until(|| kinds(&jobs) == vec!["started"]).await;
    running.shutdown.cancel();
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(!noted.load(std::sync::atomic::Ordering::SeqCst));
    let report = running.done.await.expect("joined").expect("started");
    assert_eq!(report.runs.len(), 1);
    assert!(report.runs[0].aborted);
    until(|| noted.load(std::sync::atomic::Ordering::SeqCst)).await;
}

#[tokio::test(start_paused = true)]
async fn given_a_run_whose_executor_returned_when_the_drain_time_ends_then_the_report_says_the_executor_is_not_running()
 {
    let jobs = ScriptedJobs::new("analyst", "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let gate = Arc::new(Notify::new());
    let lookup_gate = Arc::clone(&gate);
    let hook = br_runner_toolkit::lifecycle_hook(move |_ended| {
        let gate = Arc::clone(&lookup_gate);
        async move { gate.notified().await }
    });
    let running = run(
        &jobs,
        |b| b.with_lifecycle_hook(hook),
        1,
        args(single(Behavior::Result("x")), false),
    );
    until(|| kinds(&jobs) == vec!["started"]).await;
    running.shutdown.cancel();
    tokio::time::sleep(Duration::from_secs(61)).await;
    let report = running.done.await.expect("joined").expect("started");
    assert_eq!(report.runs.len(), 1);
    assert!(report.runs[0].aborted);
    assert!(!report.runs[0].executor_running);
    assert_eq!(report.runs[0].instances_without_return.len(), 1);
}
