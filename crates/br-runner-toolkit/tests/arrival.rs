mod support;

use std::sync::Arc;
use std::time::Duration;

use br_runner_jobs::test_double::{Call, ScriptedJobs};
use br_runner_jobs::{ArrivalSettings, Error};
use serde_json::json;
use support::*;
use tokio::sync::Notify;
use tracing::Level;

fn nothing_unread() -> serde_json::Value {
    json!({ "jobsUnread": { "success": true, "items": [] } })
}

#[tokio::test(start_paused = true)]
async fn given_an_arrival_that_cannot_be_decoded_when_a_run_is_in_flight_then_one_error_is_logged_and_the_run_completes()
 {
    let (logs, _guard) = Logs::capture();
    let gate = Arc::new(Notify::new());
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(nothing_unread());
    jobs.push_arrival(Err(Error::Unreadable("a bad item".to_owned())));
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    until(|| logs.containing(Level::ERROR, "cannot be decoded") == 1).await;
    gate.notify_one();
    until(|| has_terminal(&jobs)).await;
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
    assert_eq!(logs.containing(Level::ERROR, "feed of the run ended"), 0);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_an_arrival_feed_that_ends_with_an_error_when_no_beat_comes_then_the_end_is_logged_and_the_feed_is_opened_again_only_after_the_beat()
 {
    let (logs, _guard) = Logs::capture();
    let gate = Arc::new(Notify::new());
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(nothing_unread());
    jobs.push_arrival(Err(Error::InvalidInput("the feed was cut".to_owned())));
    jobs.push_trigger(trigger(1));
    let running = run(
        &jobs,
        |b| b,
        1,
        args(single(Behavior::UntilReleased(gate.clone(), "x")), false),
    );
    until(|| logs.containing(Level::ERROR, "feed of the run ended") == 1).await;
    assert_eq!(logs.containing(Level::ERROR, "cannot be opened"), 0);
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(logs.containing(Level::ERROR, "cannot be opened"), 0);
    until(|| logs.containing(Level::ERROR, "cannot be opened") == 1).await;
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(logs.containing(Level::ERROR, "cannot be opened"), 1);
    gate.notify_one();
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_second_run_whose_arrival_feed_cannot_be_opened_then_the_error_is_logged_and_the_run_completes()
 {
    let (logs, _guard) = Logs::capture();
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(nothing_unread());
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| has_terminal(&jobs)).await;
    jobs.push_trigger(trigger(2));
    until(|| kinds(&jobs) == vec!["started", "completed", "started", "completed"]).await;
    assert_eq!(logs.containing(Level::ERROR, "cannot be opened"), 1);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_run_when_its_arrival_feed_is_opened_then_the_consumer_gets_the_wait_and_the_limit_of_the_crate()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.graphql_default(nothing_unread());
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| has_terminal(&jobs)).await;
    let opened: Vec<ArrivalSettings> = jobs
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::ArrivalFeed(settings) => Some(settings),
            _ => None,
        })
        .collect();
    assert_eq!(
        opened,
        vec![ArrivalSettings {
            ack_wait: Duration::from_secs(10),
            inactivity_limit: Duration::from_secs(120),
        }]
    );
    running.stop().await;
}
