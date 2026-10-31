use super::support::*;
use crate::queue::{Gate, QueueReport, Removal, WriteKind};

#[tokio::test]
async fn given_a_held_marker_when_other_writes_are_handed_over_then_they_are_delivered_and_the_marker_waits_outside_the_order()
 {
    let rig = Rig::new();
    let (gate, mut marker_end) = rig.hold(marker(30, 10), None);
    let first = rig.hand(answer());
    let second = rig.hand(saved());
    assert_eq!(first.await, Ok(()));
    assert_eq!(second.await, Ok(()));
    settle().await;
    assert!(ready(&mut marker_end).is_none());
    assert!(rig.sender.attempts_of(WriteKind::ReadMarker).is_empty());
    let waiting: Vec<_> = rig.queue.state().waiting.iter().map(|n| n.kind).collect();
    assert_eq!(waiting, vec![WriteKind::ReadMarker]);
    gate.send(Gate::Stored).expect("awaited");
    assert_eq!(marker_end.await, Ok(()));
}

#[tokio::test]
async fn given_a_held_marker_and_an_idle_queue_when_the_gate_ends_stored_then_the_task_sends_the_marker()
 {
    let mut rig = Rig::new();
    let (gate, marker_end) = rig.hold(marker(30, 10), None);
    assert_eq!((&mut rig.started).await, Ok(()));
    settle().await;
    let (late_gate, late_end) = rig.hold(marker(31, 10), None);
    settle().await;
    gate.send(Gate::Stored).expect("awaited");
    late_gate.send(Gate::Stored).expect("awaited");
    assert_eq!(marker_end.await, Ok(()));
    assert_eq!(late_end.await, Ok(()));
    assert_eq!(
        rig.sender.attempts_of(WriteKind::ReadMarker),
        vec![marker(30, 10), marker(31, 10)]
    );
}

#[tokio::test]
async fn given_a_held_marker_with_a_follow_when_the_gate_ends_stored_then_the_follow_is_sent_before_the_marker()
 {
    let rig = Rig::new();
    let (gate, marker_end) = rig.hold(marker(30, 10), Some(finish(41, 42)));
    settle().await;
    assert_eq!(rig.sender.kinds(), vec![WriteKind::Started]);
    gate.send(Gate::Stored).expect("awaited");
    assert_eq!(marker_end.await, Ok(()));
    assert_eq!(
        rig.sender.kinds(),
        vec![WriteKind::Started, WriteKind::Finish, WriteKind::ReadMarker]
    );
}

#[tokio::test]
async fn given_a_gate_that_ends_stored_when_other_writes_wait_then_the_marker_joins_the_end_of_the_queue_at_that_moment()
 {
    let sender = ScriptedSender::new();
    let latch = Latch::new();
    sender.script(WriteKind::Answer, vec![Step::After(latch.clone(), Ok(()))]);
    let rig = Rig::with(sender);
    let a = rig.hand(answer());
    let (gate, marker_end) = rig.hold(marker(30, 10), None);
    let b = rig.hand(saved());
    settle().await;
    gate.send(Gate::Stored).expect("awaited");
    settle().await;
    let c = rig.hand(plan());
    latch.open();
    for end in [a, b, c, marker_end] {
        assert_eq!(end.await, Ok(()));
    }
    assert_eq!(
        rig.sender.kinds(),
        vec![
            WriteKind::Started,
            WriteKind::Answer,
            WriteKind::SavedState,
            WriteKind::ReadMarker,
            WriteKind::PlanDeclared
        ]
    );
}

#[tokio::test]
async fn given_a_gate_that_ends_dropped_when_it_ends_then_the_marker_is_removed_as_depending_on_a_removed_write_reported_and_never_sent()
 {
    let rig = Rig::new();
    let (gate, marker_end) = rig.hold(marker(30, 10), Some(finish(41, 42)));
    settle().await;
    assert_eq!(rig.queue.state().waiting.len(), 1);
    gate.send(Gate::Dropped).expect("awaited");
    assert_eq!(marker_end.await, Err(Removal::DependsOnRemoved));
    assert_eq!(
        rig.observer.removed(),
        vec![(marker(30, 10).name(), Removal::DependsOnRemoved)]
    );
    assert_eq!(rig.sender.kinds(), vec![WriteKind::Started]);
    assert!(rig.queue.state().waiting.is_empty());
}

#[tokio::test]
async fn given_a_held_marker_when_drained_is_called_then_it_returns_only_after_the_gate_ended_for_either_result()
 {
    for result in [Gate::Stored, Gate::Dropped] {
        let rig = Rig::new();
        let (gate, marker_end) = rig.hold(marker(30, 10), None);
        let mut drained = Box::pin(rig.queue.drained());
        assert!(futures::FutureExt::now_or_never(&mut drained).is_none());
        settle().await;
        assert!(futures::FutureExt::now_or_never(&mut drained).is_none());
        gate.send(result).expect("awaited");
        let state = drained.await;
        assert!(state.waiting.is_empty());
        assert_eq!(marker_end.await.is_ok(), result == Gate::Stored);
    }
}

#[tokio::test]
async fn given_two_held_markers_when_the_second_gate_ends_first_then_the_second_marker_is_sent_first()
 {
    let rig = Rig::new();
    let (gate_one, one) = rig.hold(marker(30, 10), None);
    let (gate_two, two) = rig.hold(marker(31, 10), None);
    settle().await;
    gate_two.send(Gate::Stored).expect("awaited");
    assert_eq!(two.await, Ok(()));
    gate_one.send(Gate::Stored).expect("awaited");
    assert_eq!(one.await, Ok(()));
    assert_eq!(
        rig.sender.attempts_of(WriteKind::ReadMarker),
        vec![marker(31, 10), marker(30, 10)]
    );
}

#[tokio::test]
async fn given_a_gate_that_has_ended_already_when_the_marker_is_held_then_it_is_sent_without_other_help()
 {
    let rig = Rig::new();
    let (gate, marker_end) = rig.hold(marker(30, 10), None);
    gate.send(Gate::Stored).expect("awaited");
    assert_eq!(marker_end.await, Ok(()));
}

#[tokio::test]
async fn given_a_held_marker_when_the_queue_state_is_read_then_the_marker_is_listed_among_the_waiting_writes_in_hand_over_order()
 {
    let sender = ScriptedSender::new();
    sender.always(WriteKind::Answer, Step::Hang);
    let rig = Rig::with(sender);
    let _a = rig.hand(answer());
    let (_gate, _end) = rig.hold(marker(30, 10), None);
    let _b = rig.hand(saved());
    let kinds: Vec<_> = rig.queue.state().waiting.iter().map(|n| n.kind).collect();
    assert_eq!(
        kinds,
        vec![
            WriteKind::Started,
            WriteKind::Answer,
            WriteKind::ReadMarker,
            WriteKind::SavedState
        ]
    );
}

#[tokio::test]
async fn given_a_marker_removed_at_its_gate_when_the_report_is_read_then_it_names_the_marker_by_item_and_agent()
 {
    let rig = Rig::new();
    let (gate, end) = rig.hold(marker(30, 10), None);
    gate.send(Gate::Dropped).expect("awaited");
    let _ = end.await;
    let reports = rig.observer.reports();
    assert!(reports.iter().any(|report| matches!(report,
        QueueReport::Removed { write, removal: Removal::DependsOnRemoved }
            if write.kind == WriteKind::ReadMarker && write.ids == vec![id(30), id(10)])));
}
