mod support;

use std::sync::Arc;
use std::time::Duration;

use br_runner_jobs::test_double::{Delivery, ScriptedJobs};
use support::*;
use tokio::sync::Notify;

#[tokio::test(start_paused = true)]
async fn given_a_duplicate_trigger_when_the_started_fact_has_not_ended_then_it_is_dropped_with_no_acknowledgement_and_starts_no_second_run()
 {
    let gate = Arc::new(Notify::new());
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.pause();
    jobs.push_trigger(trigger(1));
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        2,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(jobs.deliveries().is_empty());
    assert!(kinds(&jobs).is_empty());
    jobs.resume();
    until(|| jobs.deliveries() == vec![Delivery::Ack]).await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert_eq!(jobs.deliveries(), vec![Delivery::Ack]);
    assert_eq!(kinds(&jobs), vec!["started"]);
    gate.notify_waiters();
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_duplicate_trigger_when_the_started_fact_has_ended_and_the_run_is_in_flight_then_it_is_acknowledged_and_dropped()
 {
    let gate = Arc::new(Notify::new());
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
    until(|| jobs.deliveries() == vec![Delivery::Ack]).await;
    jobs.push_trigger(trigger(1));
    until(|| jobs.deliveries() == vec![Delivery::Ack, Delivery::Ack]).await;
    assert_eq!(kinds(&jobs), vec!["started"]);
    gate.notify_waiters();
    running.stop().await;
}
