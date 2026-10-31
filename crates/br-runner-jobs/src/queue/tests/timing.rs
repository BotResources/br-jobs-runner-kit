use std::time::Duration;

use super::support::*;
use crate::error::{Cause, Error};
use crate::queue::WriteKind;

const OUTCOME: Duration = Duration::from_secs(30);
const RETRY: Duration = Duration::from_secs(7);

fn timed(sender: std::sync::Arc<ScriptedSender>) -> Rig {
    Rig::timed(sender, OUTCOME, RETRY)
}

#[tokio::test(start_paused = true)]
async fn given_failed_attempts_when_a_write_is_sent_again_then_each_resend_waits_exactly_the_retry_wait()
 {
    let sender = ScriptedSender::new();
    sender.script(
        WriteKind::Answer,
        vec![fail(unavailable()), fail(lost()), fail(unreadable())],
    );
    let rig = timed(sender);
    assert_eq!(rig.hand(answer()).await, Ok(()));
    let times: Vec<_> = rig
        .sender
        .attempts()
        .iter()
        .zip(rig.sender.times())
        .filter(|(write, _)| write.name().kind == WriteKind::Answer)
        .map(|(_, at)| at)
        .collect();
    assert_eq!(times.len(), 4);
    for pair in times.windows(2) {
        assert_eq!(pair[1] - pair[0], RETRY);
    }
}

#[tokio::test(start_paused = true)]
async fn given_a_write_that_fails_a_very_long_time_when_it_keeps_failing_then_there_is_no_maximum()
{
    let sender = ScriptedSender::new();
    sender.script(WriteKind::Answer, vec![fail(unavailable()); 1000]);
    let rig = timed(sender);
    let start = tokio::time::Instant::now();
    assert_eq!(rig.hand(answer()).await, Ok(()));
    assert_eq!(rig.sender.attempts_of(WriteKind::Answer).len(), 1001);
    assert_eq!(start.elapsed(), RETRY * 1000);
    assert_eq!(rig.observer.retrying().len(), 1000);
}

#[tokio::test(start_paused = true)]
async fn given_a_send_without_outcome_when_the_outcome_wait_passes_then_the_attempt_is_lost_and_sent_again_after_the_retry_wait()
 {
    let sender = ScriptedSender::new();
    sender.script(WriteKind::SavedState, vec![Step::Hang, Step::Hang]);
    let rig = timed(sender);
    assert_eq!(rig.hand(saved()).await, Ok(()));
    let times: Vec<_> = rig
        .sender
        .attempts()
        .iter()
        .zip(rig.sender.times())
        .filter(|(write, _)| write.name().kind == WriteKind::SavedState)
        .map(|(_, at)| at)
        .collect();
    assert_eq!(times.len(), 3);
    assert_eq!(times[1] - times[0], OUTCOME + RETRY);
    assert_eq!(times[2] - times[1], OUTCOME + RETRY);
    let retrying = rig.observer.retrying();
    assert_eq!(retrying.len(), 2);
    for (_, error) in retrying {
        assert!(matches!(&error, Error::Lost(failure) if failure.cause == Cause::WaitFired));
    }
    assert_eq!(rig.sender.cancelled(), 2);
}

#[tokio::test(start_paused = true)]
async fn given_a_started_fact_without_outcome_when_the_wait_passes_then_it_is_sent_again_with_the_same_start_id()
 {
    let sender = ScriptedSender::new();
    sender.script(WriteKind::Started, vec![Step::Hang]);
    let rig = timed(sender);
    assert_eq!(rig.started.await, Ok(()));
    let sent = rig.sender.attempts_of(WriteKind::Started);
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0], sent[1]);
}

#[tokio::test(start_paused = true)]
async fn given_an_attempt_that_answers_in_time_when_the_outcome_wait_is_long_then_it_is_not_lost() {
    let sender = ScriptedSender::new();
    let latch = Latch::new();
    sender.script(WriteKind::Answer, vec![Step::After(latch.clone(), Ok(()))]);
    let rig = timed(sender);
    let end = rig.hand(answer());
    tokio::time::sleep(OUTCOME - Duration::from_secs(1)).await;
    latch.open();
    assert_eq!(end.await, Ok(()));
    assert_eq!(rig.sender.attempts_of(WriteKind::Answer).len(), 1);
}
