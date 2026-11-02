mod support;

use br_runner_jobs::PresenceStatus;
use br_runner_jobs::test_double::{Delivery, ScriptedJobs};
use support::*;

#[tokio::test(start_paused = true)]
async fn given_a_trigger_in_push_mode_when_the_graph_ends_then_started_acknowledged_completed_in_order()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        2,
        args(single(Behavior::Result("done")), false),
    );
    until(|| has_terminal(&jobs)).await;
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
    assert_eq!(jobs.deliveries(), vec![Delivery::Ack]);
    let documents: Vec<String> = jobs
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            br_runner_jobs::test_double::Call::Graphql { document, .. } => Some(document),
            _ => None,
        })
        .collect();
    assert!(
        documents
            .iter()
            .all(|document| document.contains("jobsUnread")),
        "{documents:?}"
    );
    let report = running.stop().await;
    assert!(report.runs.is_empty());
    assert_eq!(jobs.presence_deletions(), 1);
    let records = jobs.presence_records();
    assert_eq!(records[0].status, PresenceStatus::Ready);
    assert_eq!(records[0].capacity, 2);
    assert_eq!(records[0].runner_version, "1.0.0");
    assert_eq!(
        records.last().expect("a record").status,
        PresenceStatus::Draining
    );
}

#[tokio::test(start_paused = true)]
async fn given_more_triggers_than_slots_when_runs_are_in_flight_then_only_slot_count_runs_start() {
    let gate = std::sync::Arc::new(tokio::sync::Notify::new());
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    for n in 1..=3 {
        jobs.push_trigger(trigger(n));
    }
    let running = run(
        &jobs,
        |b| b,
        2,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    until(|| kinds(&jobs).len() >= 2).await;
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert_eq!(kinds(&jobs), vec!["started", "started"]);
    assert_eq!(jobs.triggers_waiting(), 1);
    gate.notify_one();
    until(|| kinds(&jobs).len() >= 4).await;
    gate.notify_waiters();
    gate.notify_one();
    until(|| kinds(&jobs).iter().filter(|k| **k == "started").count() == 3).await;
    gate.notify_waiters();
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_trigger_of_a_run_in_flight_when_it_arrives_again_then_it_is_acknowledged_and_dropped()
 {
    let gate = std::sync::Arc::new(tokio::sync::Notify::new());
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        2,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    until(|| kinds(&jobs) == vec!["started"]).await;
    jobs.push_trigger(trigger(1));
    until(|| jobs.deliveries().len() == 2).await;
    assert_eq!(jobs.deliveries(), vec![Delivery::Ack, Delivery::Ack]);
    assert_eq!(kinds(&jobs), vec!["started"]);
    gate.notify_one();
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
}
