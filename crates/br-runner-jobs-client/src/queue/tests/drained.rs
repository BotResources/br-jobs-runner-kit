use std::time::{Duration, Instant};

use futures::FutureExt;

use super::support::*;
use crate::error::Error;
use crate::queue::{Gate, Removal, WriteKind};

#[tokio::test]
async fn given_an_idle_queue_when_drained_is_called_then_it_returns_at_once_and_holds_nothing() {
    let rig = Rig::new();
    assert_eq!(rig.started.await, Ok(()));
    let state = rig.queue.drained().now_or_never().expect("at once");
    assert!(state.waiting.is_empty());
    assert!(state.oldest_handed_over.is_none());
    assert!(state.last_failure.is_none());
}

#[tokio::test]
async fn given_writes_handed_over_before_the_call_when_drained_waits_then_it_returns_when_they_ended_and_not_for_later_ones()
 {
    let sender = ScriptedSender::new();
    let first = Latch::new();
    let second = Latch::new();
    sender.script(WriteKind::Answer, vec![Step::After(first.clone(), Ok(()))]);
    sender.script(
        WriteKind::SavedState,
        vec![Step::After(second.clone(), Ok(()))],
    );
    let rig = Rig::with(sender);
    let a = rig.hand(answer());
    let mut drained = Box::pin(rig.queue.drained());
    assert!(drained.as_mut().now_or_never().is_none());
    let b = rig.hand(saved());
    first.open();
    let state = drained.await;
    assert_eq!(a.await, Ok(()));
    let waiting: Vec<_> = state.waiting.iter().map(|n| n.kind).collect();
    assert_eq!(waiting, vec![WriteKind::SavedState]);
    second.open();
    assert_eq!(b.await, Ok(()));
}

#[tokio::test]
async fn given_an_abandoned_drained_call_when_it_is_dropped_and_called_again_then_nothing_is_lost()
{
    let sender = ScriptedSender::new();
    let latch = Latch::new();
    sender.script(WriteKind::Answer, vec![Step::After(latch.clone(), Ok(()))]);
    let rig = Rig::with(sender);
    let end = rig.hand(answer());
    for _ in 0..3 {
        let mut abandoned = Box::pin(rig.queue.drained());
        assert!(abandoned.as_mut().now_or_never().is_none());
        settle().await;
        assert!(abandoned.as_mut().now_or_never().is_none());
        drop(abandoned);
    }
    let again = rig.queue.drained();
    latch.open();
    let state = again.await;
    assert!(state.waiting.is_empty());
    assert_eq!(end.await, Ok(()));
}

#[tokio::test]
async fn given_several_drained_calls_at_once_when_the_writes_end_then_every_call_returns() {
    let sender = ScriptedSender::new();
    let latch = Latch::new();
    sender.script(WriteKind::Answer, vec![Step::After(latch.clone(), Ok(()))]);
    let rig = Rig::with(sender);
    let _end = rig.hand(answer());
    let calls = futures::future::join_all((0..5).map(|_| rig.queue.drained()));
    let mut calls = Box::pin(calls);
    assert!(calls.as_mut().now_or_never().is_none());
    latch.open();
    assert_eq!(calls.await.len(), 5);
}

#[tokio::test]
async fn given_a_removed_write_when_drained_is_called_then_the_removal_counts_as_an_end() {
    let sender = ScriptedSender::new();
    sender.script(WriteKind::Answer, vec![fail(Error::Unauthenticated)]);
    let rig = Rig::with(sender);
    let end = rig.hand(answer());
    let state = rig.queue.drained().await;
    assert!(state.waiting.is_empty());
    assert_eq!(end.await, Err(Removal::Failed(Error::Unauthenticated)));
}

#[tokio::test]
async fn given_a_write_removed_at_the_hand_over_when_drained_is_called_then_it_does_not_wait() {
    let sender = ScriptedSender::new();
    sender.script(WriteKind::Upload, vec![fail(Error::Unauthenticated)]);
    let rig = Rig::with(sender);
    let _ = rig.hand(upload(41)).await;
    let mut late = rig.hand(create(41, 42));
    assert!(ready(&mut late).is_some());
    assert!(rig.queue.drained().now_or_never().is_some());
}

#[tokio::test]
async fn given_waiting_writes_when_the_state_is_read_then_they_are_named_in_hand_over_order_with_the_oldest_instant()
 {
    let sender = ScriptedSender::new();
    sender.always(WriteKind::Answer, Step::Hang);
    let before = Instant::now();
    let rig = Rig::with(sender);
    let _a = rig.hand(answer());
    let _b = rig.hand(saved());
    let _c = rig.hand(mail(20, 99, 98));
    let after = Instant::now();
    let state = rig.queue.state();
    let names: Vec<_> = state
        .waiting
        .iter()
        .map(|n| (n.kind, n.ids.clone()))
        .collect();
    assert_eq!(
        names,
        vec![
            (WriteKind::Started, vec![id(RUN), id(START)]),
            (WriteKind::Answer, vec![]),
            (WriteKind::SavedState, vec![]),
            (WriteKind::Mail, vec![id(20), id(99)]),
        ]
    );
    let oldest = state.oldest_handed_over.expect("writes wait");
    assert!(before <= oldest && oldest <= after);
}

#[tokio::test]
async fn given_the_oldest_write_ended_when_the_state_is_read_then_the_oldest_instant_moves_to_the_next()
 {
    let sender = ScriptedSender::new();
    let latch = Latch::new();
    sender.script(WriteKind::Answer, vec![Step::After(latch.clone(), Ok(()))]);
    sender.always(WriteKind::SavedState, Step::Hang);
    let mut rig = Rig::with(sender);
    assert_eq!((&mut rig.started).await, Ok(()));
    let a = rig.hand(answer());
    let first = rig.queue.state().oldest_handed_over.expect("waits");
    std::thread::sleep(Duration::from_millis(2));
    let _b = rig.hand(saved());
    latch.open();
    assert_eq!(a.await, Ok(()));
    let second = rig.queue.state().oldest_handed_over.expect("waits");
    assert!(second > first);
}

#[tokio::test]
async fn given_a_failing_write_when_the_state_is_read_then_it_gives_the_last_failed_attempt_and_clears_it_when_the_write_ends()
 {
    let sender = ScriptedSender::new();
    let latch = Latch::new();
    sender.script(
        WriteKind::Answer,
        vec![
            fail(unavailable()),
            fail(lost()),
            Step::After(latch.clone(), Ok(())),
        ],
    );
    let rig = Rig::with(sender);
    let end = rig.hand(answer());
    rig.sender.attempts_reached(4).await;
    settle().await;
    let failure = rig.queue.state().last_failure.expect("a failed attempt");
    assert_eq!(failure.write.kind, WriteKind::Answer);
    assert_eq!(failure.error, lost());
    latch.open();
    assert_eq!(end.await, Ok(()));
    assert!(rig.queue.state().last_failure.is_none());
}

#[tokio::test]
async fn given_a_failing_write_that_ends_removed_when_the_state_is_read_then_the_last_failure_is_cleared()
 {
    let sender = ScriptedSender::new();
    sender.script(
        WriteKind::Answer,
        vec![fail(unavailable()), fail(Error::Unauthenticated)],
    );
    let rig = Rig::with(sender);
    let end = rig.hand(answer());
    assert!(end.await.is_err());
    assert!(rig.queue.state().last_failure.is_none());
}

#[tokio::test]
async fn given_a_held_marker_and_a_gate_when_the_state_is_read_before_and_after_then_the_marker_counts_until_its_end()
 {
    let rig = Rig::new();
    let (gate, end) = rig.hold(marker(30, 10), None);
    assert_eq!(rig.queue.state().waiting.len(), 2);
    gate.send(Gate::Stored).expect("awaited");
    assert_eq!(end.await, Ok(()));
    assert!(rig.queue.drained().await.waiting.is_empty());
}
