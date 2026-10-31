use futures::FutureExt;

use super::support::*;
use crate::queue::{Gate, Removal, WriteKind, WriteName};

fn removals(rig: &Rig) -> Vec<(WriteKind, Removal)> {
    rig.observer
        .removed()
        .into_iter()
        .map(|(name, removal)| (name.kind, removal))
        .collect()
}

#[tokio::test]
async fn given_in_flight_queued_and_held_writes_when_the_queue_is_dropped_then_each_is_reported_unsent_in_hand_over_order()
 {
    let sender = ScriptedSender::new();
    sender.always(WriteKind::Answer, Step::Hang);
    let mut rig = Rig::with(sender);
    assert_eq!((&mut rig.started).await, Ok(()));
    let a = rig.hand(answer());
    let b = rig.hand(saved());
    let (gate, held) = rig.hold(marker(30, 10), Some(finish(41, 42)));
    let c = rig.hand(plan());
    settle().await;
    let Rig {
        queue,
        sender,
        observer,
        ..
    } = rig;
    drop(queue);
    for end in [a, b, held, c] {
        assert_eq!(end.await, Err(Removal::LeftUnsent));
    }
    assert!(gate.send(Gate::Stored).is_err());
    let order: Vec<_> = observer
        .removed()
        .into_iter()
        .map(|(name, _)| name.kind)
        .collect();
    assert_eq!(
        order,
        vec![
            WriteKind::Answer,
            WriteKind::SavedState,
            WriteKind::ReadMarker,
            WriteKind::PlanDeclared
        ]
    );
    assert!(
        observer
            .removed()
            .iter()
            .all(|(_, removal)| *removal == Removal::LeftUnsent)
    );
    assert_eq!(sender.cancelled(), 1);
}

#[tokio::test]
async fn given_a_dropped_queue_when_a_write_is_handed_over_through_a_writer_then_it_ends_at_once_unsent_and_is_reported()
 {
    for yielded in [false, true] {
        let rig = Rig::new();
        let writer = rig.queue.writer();
        let Rig {
            queue,
            observer,
            sender,
            ..
        } = rig;
        drop(queue);
        if yielded {
            settle().await;
        }
        let mut late = writer.hand_over(answer());
        assert_eq!(ready(&mut late), Some(Err(Removal::LeftUnsent)));
        let mut held = writer.hold(marker(30, 10), None, gate().1);
        assert_eq!(ready(&mut held), Some(Err(Removal::LeftUnsent)));
        assert!(observer.removed().iter().any(
            |(name, removal)| name.kind == WriteKind::Answer && *removal == Removal::LeftUnsent
        ));
        assert!(
            observer
                .removed()
                .iter()
                .any(|(name, _)| name.kind == WriteKind::ReadMarker)
        );
        let waiting = writer.shared.snapshot().waiting;
        assert!(waiting.iter().all(|name| name.kind == WriteKind::Started));
        assert!(!sender.kinds().contains(&WriteKind::Answer));
    }
}

#[tokio::test]
async fn given_a_dropped_queue_when_drained_is_called_then_it_returns_with_nothing_held() {
    let sender = ScriptedSender::new();
    sender.always(WriteKind::Answer, Step::Hang);
    let rig = Rig::with(sender);
    let _end = rig.hand(answer());
    let writer = rig.queue.writer();
    drop(rig.queue);
    let state = writer.shared.drained().await;
    assert!(state.waiting.is_empty());
}

#[tokio::test]
async fn given_a_held_marker_when_the_queue_is_dropped_then_its_gate_is_dropped_and_no_finish_or_marker_is_made()
 {
    let rig = Rig::new();
    let (tx, gate_future, gate_dropped) = tracked_gate();
    let held = rig
        .queue
        .writer()
        .hold(marker(30, 10), Some(finish(41, 42)), gate_future);
    settle().await;
    assert!(!gate_dropped.load(std::sync::atomic::Ordering::SeqCst));
    let Rig { queue, sender, .. } = rig;
    drop(queue);
    assert_eq!(held.await, Err(Removal::LeftUnsent));
    assert!(gate_dropped.load(std::sync::atomic::Ordering::SeqCst));
    assert!(tx.send(Gate::Stored).is_err());
    settle().await;
    assert_eq!(sender.kinds(), vec![WriteKind::Started]);
}

#[tokio::test]
async fn given_a_gate_that_ends_in_the_same_moment_as_the_drop_when_the_task_runs_then_the_drop_wins_and_nothing_is_made()
 {
    let rig = Rig::new();
    let (tx, held) = rig.hold(marker(30, 10), Some(finish(41, 42)));
    settle().await;
    tx.send(Gate::Stored).expect("awaited");
    let Rig {
        queue,
        sender,
        observer,
        ..
    } = rig;
    drop(queue);
    assert_eq!(held.await, Err(Removal::LeftUnsent));
    settle().await;
    assert_eq!(sender.kinds(), vec![WriteKind::Started]);
    assert_eq!(
        observer
            .removed()
            .iter()
            .filter(|(name, _)| name.kind == WriteKind::ReadMarker)
            .count(),
        1
    );
}

#[tokio::test]
async fn given_a_ready_gate_when_the_queue_is_dropped_before_the_task_ever_runs_then_the_marker_is_unsent()
 {
    let rig = Rig::new();
    let (tx, held) = rig.hold(marker(30, 10), Some(finish(41, 42)));
    tx.send(Gate::Stored).expect("awaited");
    let Rig { queue, sender, .. } = rig;
    drop(queue);
    assert_eq!(held.await, Err(Removal::LeftUnsent));
    assert!(sender.attempts_of(WriteKind::ReadMarker).is_empty());
    assert!(sender.attempts_of(WriteKind::Finish).is_empty());
}

#[tokio::test]
async fn given_a_write_that_keeps_failing_when_the_queue_is_dropped_then_the_attempts_stop() {
    let sender = ScriptedSender::new();
    sender.always(WriteKind::Answer, fail(unavailable()));
    let rig = Rig::with(sender);
    let end = rig.hand(answer());
    rig.sender.attempts_reached(5).await;
    let Rig { queue, sender, .. } = rig;
    drop(queue);
    assert_eq!(end.await, Err(Removal::LeftUnsent));
    let count = sender.attempts().len();
    settle().await;
    assert_eq!(sender.attempts().len(), count);
}

#[tokio::test]
async fn given_a_dropped_queue_when_the_state_is_read_through_a_writer_then_nothing_waits_and_the_failure_is_cleared()
 {
    let sender = ScriptedSender::new();
    sender.always(WriteKind::Answer, fail(lost()));
    let rig = Rig::with(sender);
    let end = rig.hand(answer());
    rig.sender.attempts_reached(3).await;
    let writer = rig.queue.writer();
    assert!(writer.shared.snapshot().last_failure.is_some());
    drop(rig.queue);
    let _ = end.await;
    let state = writer.shared.snapshot();
    assert!(state.waiting.is_empty() && state.last_failure.is_none());
}

#[test]
fn given_the_runtime_shut_down_when_the_queue_holds_writes_then_they_end_unsent_are_reported_and_drained_returns()
 {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("a runtime");
    let sender = ScriptedSender::new();
    sender.always(WriteKind::Answer, Step::Hang);
    let (rig, end, held, flag) = runtime.block_on(async {
        let rig = Rig::with(sender);
        let end = rig.hand(answer());
        let (_tx, gate_future, flag) = tracked_gate();
        let held = rig.queue.writer().hold(marker(30, 10), None, gate_future);
        settle().await;
        (rig, end, held, flag)
    });
    drop(runtime);
    assert_eq!(end.now_or_never(), Some(Err(Removal::LeftUnsent)));
    assert_eq!(held.now_or_never(), Some(Err(Removal::LeftUnsent)));
    assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
    assert!(rig.queue.drained().now_or_never().is_some());
    let kinds: Vec<WriteName> = rig
        .observer
        .removed()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(kinds.iter().any(|name| name.kind == WriteKind::Answer));
    assert!(kinds.iter().any(|name| name.kind == WriteKind::ReadMarker));
}

#[tokio::test]
async fn given_a_sender_that_panics_when_the_task_dies_then_every_write_ends_unsent_and_drained_returns()
 {
    let sender = ScriptedSender::new();
    sender.script(WriteKind::Answer, vec![Step::Panic]);
    let rig = Rig::with(sender);
    let a = rig.hand(answer());
    let b = rig.hand(saved());
    assert_eq!(a.await, Err(Removal::LeftUnsent));
    assert_eq!(b.await, Err(Removal::LeftUnsent));
    assert!(rig.queue.drained().await.waiting.is_empty());
    let mut late = rig.hand(plan());
    assert_eq!(ready(&mut late), Some(Err(Removal::LeftUnsent)));
    assert_eq!(removals(&rig).len(), 3);
}
