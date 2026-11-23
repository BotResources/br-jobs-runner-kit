use std::time::Duration;

use super::*;
use crate::error::Error;
use crate::queue::IgnoreReports;

#[tokio::test]
async fn given_a_queue_made_when_a_second_is_made_from_the_same_work_then_invalid_input() {
    let jobs = jobs();
    let work = work(&jobs);
    let _first = work.delivery_queue(settings()).await.expect("a queue");
    assert!(matches!(
        work.delivery_queue(settings()).await,
        Err(Error::InvalidInput(_))
    ));
}

#[tokio::test]
async fn given_a_queue_made_when_it_starts_then_its_first_write_is_the_started_fact_with_the_start_id()
 {
    let jobs = jobs();
    let work = work(&jobs);
    let (queue, started) = work.delivery_queue(settings()).await.expect("a queue");
    started.await.expect("started");
    drop(queue);
    assert!(matches!(
        jobs.calls().first(),
        Some(crate::test_double::Call::Started { run_id, start_id })
            if *run_id == id(RUN) && *start_id == id(START)
    ));
}

#[tokio::test]
async fn given_a_zero_wait_when_the_queue_is_made_then_invalid_input_and_a_valid_making_after_it_still_succeeds()
 {
    let jobs = jobs();
    let work = work(&jobs);
    for (outcome_wait, retry_wait) in [
        (Duration::ZERO, Duration::from_millis(10)),
        (Duration::from_secs(1), Duration::ZERO),
        (Duration::ZERO, Duration::ZERO),
    ] {
        let refused = work
            .delivery_queue(QueueSettings {
                start_id: id(START),
                outcome_wait,
                retry_wait,
                observer: Arc::new(IgnoreReports),
            })
            .await;
        assert!(matches!(refused, Err(Error::InvalidInput(_))));
    }
    assert!(work.delivery_queue(settings()).await.is_ok());
}

#[tokio::test]
async fn given_two_works_when_each_makes_its_queue_then_each_has_its_own_once() {
    let jobs = jobs();
    let first = work(&jobs);
    let second = work(&jobs);
    assert!(first.delivery_queue(settings()).await.is_ok());
    assert!(second.delivery_queue(settings()).await.is_ok());
}

#[tokio::test]
async fn given_a_book_made_when_a_second_is_made_then_invalid_input_and_the_queue_stays_usable() {
    let jobs = jobs();
    let work = work(&jobs);
    let (queue, _) = work.delivery_queue(settings()).await.expect("a queue");
    work.agent_book(&queue, Vec::new(), false).expect("a book");
    assert!(matches!(
        work.agent_book(&queue, Vec::new(), false),
        Err(Error::InvalidInput(_))
    ));
    queue.completed().await.expect("delivered");
}

#[tokio::test]
async fn given_a_work_when_the_mail_handle_is_taken_twice_then_both_work() {
    let jobs = jobs();
    let work = work(&jobs);
    work.mail_handle().list_agents().await.expect("first");
    work.mail_handle().list_agents().await.expect("second");
}

#[test]
fn given_no_async_runtime_when_the_queue_is_made_then_invalid_input_and_the_once_is_not_spent() {
    let jobs = jobs();
    let work = work(&jobs);
    let refused = futures::executor::block_on(work.delivery_queue(settings()));
    assert!(matches!(refused, Err(Error::InvalidInput(_))));
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    runtime.block_on(async {
        assert!(work.delivery_queue(settings()).await.is_ok());
    });
}
