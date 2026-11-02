mod support;

use std::time::Duration;

use br_runner_jobs::test_double::ScriptedJobs;
use br_runner_jobs::{CancelEvent, DispatchMode, Error, ModeEvent};
use support::*;

fn feed_error() -> Error {
    Error::InfraMissing("the feed".to_owned())
}

#[tokio::test(start_paused = true)]
async fn given_a_trigger_feed_that_ends_with_an_error_when_a_trigger_is_made_then_no_slot_opens_the_feed_again_before_the_next_beat()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    let running = run(&jobs, |b| b, 2, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(1)).await;
    jobs.push_trigger_error(feed_error());
    tokio::time::sleep(Duration::from_secs(1)).await;
    jobs.push_trigger(trigger(1));
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert!(kinds(&jobs).is_empty());
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_trigger_that_cannot_be_decoded_when_it_comes_then_it_is_skipped_and_the_next_trigger_runs()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_trigger(b"{".to_vec());
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| has_terminal(&jobs)).await;
    assert_eq!(kinds(&jobs), vec!["started", "completed"]);
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_dispatch_mode_feed_that_ends_with_an_error_when_a_trigger_is_made_then_the_runner_keeps_the_last_mode()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_mode(Err(feed_error()));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(40)).await;
    jobs.push_trigger(trigger(1));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_dispatch_mode_event_that_cannot_be_decoded_when_it_comes_then_the_mode_does_not_change()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    jobs.push_mode(Err(Error::Unreadable("not json".to_owned())));
    jobs.push_trigger(trigger(1));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| has_terminal(&jobs)).await;
    running.stop().await;
}

#[tokio::test(start_paused = true)]
async fn given_a_cancel_feed_that_ends_with_an_error_when_a_trigger_is_made_then_the_runner_asks_for_no_trigger_until_the_replay_is_complete_again()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    push_mode(&jobs);
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    tokio::time::sleep(Duration::from_secs(1)).await;
    jobs.push_cancel(Err(feed_error()));
    tokio::time::sleep(Duration::from_secs(1)).await;
    jobs.push_trigger(trigger(1));
    tokio::time::sleep(Duration::from_secs(300)).await;
    assert!(kinds(&jobs).is_empty());
    running.shutdown.cancel();
    let _ = running.done.await;
}

#[tokio::test(start_paused = true)]
async fn given_cancel_orders_made_and_deleted_before_the_replay_is_complete_when_the_replay_ends_then_the_replay_gives_the_orders_that_stand()
 {
    let jobs = ScriptedJobs::new(runner_type("analyst"), "i1");
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Push)));
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(101) }));
    jobs.push_cancel(Ok(CancelEvent::OrderDeleted { run_id: id(101) }));
    jobs.push_cancel(Ok(CancelEvent::OrderMade { run_id: id(102) }));
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
    jobs.push_trigger(trigger(1));
    jobs.push_trigger(trigger(2));
    let running = run(&jobs, |b| b, 1, args(single(Behavior::Result("x")), false));
    until(|| has_terminal(&jobs)).await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(kinds(&jobs), vec!["started", "completed", "started"]);
    running.stop().await;
}
