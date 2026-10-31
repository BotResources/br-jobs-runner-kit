use std::time::Duration;

use futures::FutureExt;
use serde_json::json;

use super::*;
use crate::client::TriggerFeed;
use crate::error::Error;
use crate::test_double::Delivery;

async fn feed_of(jobs: &Arc<ScriptedJobs>) -> TriggerFeed {
    jobs.client()
        .triggers(Duration::from_secs(30))
        .await
        .expect("a trigger feed")
}

fn bytes(value: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("bytes")
}

async fn next_work(feed: &mut TriggerFeed) -> Work {
    feed.next()
        .await
        .map_err(|unreadable| unreadable.error.to_string())
        .expect("a Work")
}

#[tokio::test]
async fn given_a_trigger_when_the_work_acks_then_the_delivery_is_acked_and_nothing_else_is_settled()
{
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    jobs.push_trigger(bytes(&trigger()));
    let work = next_work(&mut feed).await;
    assert_eq!(work.run_id(), id(RUN));
    work.ack().await.expect("acked");
    assert_eq!(jobs.deliveries(), vec![Delivery::Ack]);
}

#[tokio::test]
async fn given_a_trigger_when_the_work_is_given_back_then_the_delivery_is_given_back() {
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    jobs.push_trigger(bytes(&trigger()));
    next_work(&mut feed)
        .await
        .give_back()
        .await
        .expect("given back");
    assert_eq!(jobs.deliveries(), vec![Delivery::GiveBack]);
}

#[tokio::test]
async fn given_a_trigger_when_the_work_is_terminated_then_the_delivery_is_terminated() {
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    jobs.push_trigger(bytes(&trigger()));
    next_work(&mut feed)
        .await
        .terminate()
        .await
        .expect("terminated");
    assert_eq!(jobs.deliveries(), vec![Delivery::Terminate]);
}

#[tokio::test]
async fn given_a_broker_that_fails_the_settle_when_the_work_settles_then_the_error_is_returned() {
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    jobs.push_trigger(bytes(&trigger()));
    let work = next_work(&mut feed).await;
    jobs.fail_next_delivery(Error::wait_fired("no confirmation"));
    assert!(matches!(work.ack().await, Err(Error::Lost(_))));
    work.ack().await.expect("the next settle is not affected");
}

#[tokio::test]
async fn given_a_trigger_that_is_not_json_when_read_then_it_is_terminated_with_no_ids_and_the_feed_goes_on()
 {
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    jobs.push_trigger(b"{".to_vec());
    jobs.push_trigger(bytes(&trigger()));
    let unreadable = feed.next().await.expect_err("unreadable");
    assert!(matches!(unreadable.error, Error::Unreadable(_)));
    assert_eq!((unreadable.job_id, unreadable.run_id), (None, None));
    assert_eq!(jobs.deliveries(), vec![Delivery::Terminate]);
    let work = next_work(&mut feed).await;
    assert_eq!(work.job_id(), id(JOB));
}

#[tokio::test]
async fn given_a_trigger_with_no_passport_when_read_then_it_is_terminated_and_the_event_carries_the_job_and_the_run()
 {
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    jobs.push_trigger(bytes(&trigger_without("passport")));
    let unreadable = feed.next().await.expect_err("unreadable");
    assert!(matches!(unreadable.error, Error::Unreadable(_)));
    assert_eq!(unreadable.job_id, Some(id(JOB)));
    assert_eq!(unreadable.run_id, Some(id(RUN)));
    assert_eq!(jobs.deliveries(), vec![Delivery::Terminate]);
}

#[tokio::test]
async fn given_a_passport_that_cannot_be_decoded_or_a_background_call_without_address_when_read_then_each_is_terminated()
 {
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    jobs.push_trigger(bytes(&trigger_with(&[(
        "passport",
        json!({ "kind": "robot" }),
    )])));
    jobs.push_trigger(bytes(&trigger_with(&[("mode", json!("BACKGROUND"))])));
    for _ in 0..2 {
        let unreadable = feed.next().await.expect_err("unreadable");
        assert_eq!(unreadable.job_id, Some(id(JOB)));
    }
    assert_eq!(jobs.deliveries(), vec![Delivery::Terminate; 2]);
}

#[tokio::test]
async fn given_a_broker_that_fails_the_terminate_when_a_trigger_is_unreadable_then_the_event_is_still_the_unreadable_one()
 {
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    jobs.push_trigger(bytes(&trigger_without("passport")));
    jobs.fail_next_delivery(Error::wait_fired("no confirmation"));
    let unreadable = feed.next().await.expect_err("unreadable");
    assert!(matches!(unreadable.error, Error::Unreadable(_)));
    assert_eq!(unreadable.job_id, Some(id(JOB)));
}

#[tokio::test]
async fn given_three_triggers_waiting_when_the_importer_asks_once_then_one_is_taken() {
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    for _ in 0..3 {
        jobs.push_trigger(bytes(&trigger()));
    }
    assert_eq!(jobs.triggers_waiting(), 3);
    let _work = next_work(&mut feed).await;
    assert_eq!(jobs.triggers_waiting(), 2);
}

#[tokio::test]
async fn given_a_next_that_was_dropped_when_a_trigger_comes_then_the_next_call_gets_it() {
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    assert!(feed.next().now_or_never().is_none());
    jobs.push_trigger(bytes(&trigger()));
    let work = next_work(&mut feed).await;
    assert_eq!(work.run_id(), id(RUN));
}

#[tokio::test]
async fn given_a_source_that_fails_when_the_feed_is_asked_again_then_the_same_error_is_repeated_and_the_source_is_not_pulled()
 {
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    jobs.push_trigger_error(Error::lost("the consumer ended"));
    jobs.push_trigger(bytes(&trigger()));
    let first = feed.next().await.expect_err("ended");
    let second = feed.next().await.expect_err("ended again");
    assert!(matches!(first.error, Error::Lost(_)));
    assert_eq!(first.error, second.error);
    assert_eq!(jobs.triggers_waiting(), 1);
}

#[tokio::test]
async fn given_an_unreadable_trigger_then_a_failing_source_when_asked_then_the_unreadable_does_not_end_the_feed_but_the_failure_does()
 {
    let jobs = jobs();
    let mut feed = feed_of(&jobs).await;
    jobs.push_trigger(b"not json".to_vec());
    jobs.push_trigger_error(Error::InfraMissing("the stream is gone".to_owned()));
    assert!(matches!(
        feed.next().await.expect_err("unreadable").error,
        Error::Unreadable(_)
    ));
    assert!(matches!(
        feed.next().await.expect_err("ended").error,
        Error::InfraMissing(_)
    ));
    assert!(matches!(
        feed.next().await.expect_err("still ended").error,
        Error::InfraMissing(_)
    ));
}

#[tokio::test]
async fn given_a_zero_ack_wait_when_the_trigger_feed_is_opened_then_invalid_input() {
    let jobs = jobs();
    let opened = jobs.client().triggers(Duration::ZERO).await;
    assert!(matches!(opened, Err(Error::InvalidInput(_))));
}

#[tokio::test]
async fn given_a_work_given_back_when_the_queue_the_handles_and_the_book_made_before_are_used_then_they_keep_working()
 {
    let jobs = jobs();
    jobs.graphql_default(json!({}));
    let mut feed = feed_of(&jobs).await;
    jobs.push_trigger(bytes(&trigger()));
    let work = next_work(&mut feed).await;
    let (queue, started) = work.delivery_queue(settings()).await.expect("a queue");
    started.await.expect("started");
    let book = work.agent_book(&queue, Vec::new(), false).expect("a book");
    let mail = work.mail_handle();
    let caller = book.caller_handle();
    work.give_back().await.expect("given back");
    assert_eq!(jobs.deliveries(), vec![Delivery::GiveBack]);
    queue.completed().await.expect("the queue still delivers");
    mail.list_agents()
        .await
        .expect("the mail handle still calls");
    let input = crate::caller::CreateChildInput::new(
        id(300),
        id(301),
        "analyst",
        crate::model::CallMode::Sync,
        crate::model::PayloadForm::Kept,
    );
    caller
        .create_child(input)
        .expect("handed over")
        .await
        .expect("the caller handle still creates");
}
