use std::sync::Arc;
use std::time::Duration;

use super::support::*;
use crate::error::{Error, Operation};
use crate::queue::{DeliveryQueue, IgnoreReports, QueueSettings, Removal, WriteKind};

fn zero(outcome: u64, retry: u64) -> QueueSettings {
    settings_with(
        Arc::new(IgnoreReports),
        Duration::from_millis(outcome),
        Duration::from_millis(retry),
    )
}

#[tokio::test]
async fn given_a_zero_outcome_wait_when_the_queue_is_made_then_it_is_refused_with_invalid_input() {
    let result = DeliveryQueue::start(ScriptedSender::new(), zero(0, 5), id(RUN));
    assert!(matches!(result, Err(Error::InvalidInput(_))));
}

#[tokio::test]
async fn given_a_zero_retry_wait_when_the_queue_is_made_then_it_is_refused_with_invalid_input() {
    let result = DeliveryQueue::start(ScriptedSender::new(), zero(5, 0), id(RUN));
    assert!(matches!(result, Err(Error::InvalidInput(_))));
}

#[tokio::test]
async fn given_a_zero_wait_when_the_queue_is_made_then_nothing_is_sent() {
    let sender = ScriptedSender::new();
    let _ = DeliveryQueue::start(sender.clone(), zero(0, 5), id(RUN));
    let _ = DeliveryQueue::start(sender.clone(), zero(5, 0), id(RUN));
    settle().await;
    assert!(sender.attempts().is_empty());
}

#[tokio::test]
async fn given_a_zero_wait_when_the_queue_is_made_from_a_work_then_it_is_refused_and_a_valid_making_still_works()
 {
    let jobs = crate::book::tests::scripted();
    let work = crate::book::tests::work(&jobs);
    for (outcome, retry) in [(0, 5), (5, 0), (0, 0)] {
        let refused = work.delivery_queue(zero(outcome, retry)).await;
        assert!(matches!(refused, Err(Error::InvalidInput(_))));
    }
    let made = work.delivery_queue(zero(5000, 5)).await;
    assert!(made.is_ok());
    let again = work.delivery_queue(zero(5000, 5)).await;
    assert!(matches!(again, Err(Error::InvalidInput(_))));
}

#[test]
fn given_no_async_runtime_when_the_queue_is_made_then_it_is_refused_not_a_panic() {
    let result = DeliveryQueue::start(ScriptedSender::new(), zero(5, 5), id(RUN));
    assert!(matches!(result, Err(Error::InvalidInput(_))));
}

#[tokio::test]
async fn given_a_queue_when_it_is_made_then_the_started_fact_is_the_first_write_and_its_end_is_returned()
 {
    let sender = ScriptedSender::new();
    let latch = Latch::new();
    sender.script(WriteKind::Started, vec![Step::After(latch.clone(), Ok(()))]);
    let mut rig = Rig::with(sender);
    let later = rig.hand(answer());
    settle().await;
    assert!(ready(&mut rig.started).is_none());
    assert_eq!(rig.sender.kinds(), vec![WriteKind::Started]);
    latch.open();
    assert_eq!(rig.started.await, Ok(()));
    assert_eq!(later.await, Ok(()));
    let first = &rig.sender.attempts()[0];
    assert_eq!(
        first.name().ids,
        vec![id(RUN), id(START)],
        "the started fact carries the run id and the start id"
    );
}

#[tokio::test]
async fn given_a_refused_started_fact_when_the_making_returns_its_end_then_it_is_refused_and_every_later_write_depends_on_it()
 {
    let sender = ScriptedSender::new();
    sender.script(
        WriteKind::Started,
        vec![fail(refusal(Operation::StartedFact, "run_not_pending"))],
    );
    let rig = Rig::with(sender);
    let later = rig.hand(answer());
    assert!(matches!(rig.started.await, Err(Removal::Refused(_))));
    assert_eq!(later.await, Err(Removal::DependsOnRemoved));
    assert_eq!(rig.sender.kinds(), vec![WriteKind::Started]);
}
