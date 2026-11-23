use futures::FutureExt;

use super::support::*;
use crate::queue::WriteKind;

#[tokio::test]
async fn given_writes_of_every_independent_kind_when_handed_over_then_they_are_sent_in_hand_over_order()
 {
    let rig = Rig::new();
    let writes = vec![
        answer(),
        step(1),
        saved(),
        plan(),
        upload(40),
        registration(10),
        step(2),
        failed_fact(),
    ];
    let ends: Vec<_> = writes.iter().cloned().map(|w| rig.hand(w)).collect();
    for end in ends {
        assert_eq!(end.await, Ok(()));
    }
    let sent = rig.sender.attempts();
    assert_eq!(&sent[1..], writes.as_slice());
}

#[tokio::test]
async fn given_a_write_in_flight_when_a_later_one_is_handed_over_then_it_is_sent_only_after_the_earlier_one_ended()
 {
    let sender = ScriptedSender::new();
    let latch = Latch::new();
    sender.script(WriteKind::Answer, vec![Step::After(latch.clone(), Ok(()))]);
    let rig = Rig::with(sender);
    let first = rig.hand(answer());
    let second = rig.hand(saved());
    settle().await;
    assert_eq!(
        rig.sender.kinds(),
        vec![WriteKind::Started, WriteKind::Answer]
    );
    latch.open();
    assert_eq!(first.await, Ok(()));
    assert_eq!(second.await, Ok(()));
    assert_eq!(
        rig.sender.kinds(),
        vec![WriteKind::Started, WriteKind::Answer, WriteKind::SavedState]
    );
}

#[tokio::test]
async fn given_a_write_that_keeps_failing_with_a_resendable_error_when_later_writes_wait_then_none_is_sent_until_it_is_delivered()
 {
    let sender = ScriptedSender::new();
    let latch = Latch::new();
    sender.script(
        WriteKind::Answer,
        vec![
            fail(unavailable()),
            fail(lost()),
            fail(unreadable()),
            fail(fetch_failed()),
            Step::After(latch.clone(), Ok(())),
        ],
    );
    let rig = Rig::with(sender);
    let first = rig.hand(answer());
    let later: Vec<_> = vec![rig.hand(saved()), rig.hand(plan()), rig.hand(step(1))];
    rig.sender.attempts_reached(6).await;
    settle().await;
    let kinds = rig.sender.kinds();
    assert_eq!(kinds.len(), 6);
    assert!(kinds[1..].iter().all(|kind| *kind == WriteKind::Answer));
    latch.open();
    assert_eq!(first.await, Ok(()));
    for end in later {
        assert_eq!(end.await, Ok(()));
    }
    assert_eq!(
        &rig.sender.kinds()[6..],
        &[
            WriteKind::SavedState,
            WriteKind::PlanDeclared,
            WriteKind::StepStarted
        ]
    );
}

#[tokio::test]
async fn given_a_hand_over_when_it_returns_then_the_end_is_not_ready_and_the_call_did_not_wait_for_the_sender()
 {
    let sender = ScriptedSender::new();
    sender.always(WriteKind::Answer, Step::Hang);
    let rig = Rig::with(sender);
    let mut end = rig.hand(answer());
    assert!(ready(&mut end).is_none());
    settle().await;
    assert!(ready(&mut end).is_none());
}

#[tokio::test]
async fn given_a_hand_over_when_the_callers_future_is_dropped_then_both_writes_of_one_enqueue_are_still_sent()
 {
    let rig = Rig::new();
    let writer = rig.queue.writer();
    let caller = async move {
        let first = writer.hand_over(answer());
        let second = writer.hand_over(saved());
        let _ends = (first, second);
        std::future::pending::<()>().await;
    };
    assert!(caller.now_or_never().is_none());
    rig.queue.drained().await;
    assert_eq!(
        rig.sender.kinds(),
        vec![WriteKind::Started, WriteKind::Answer, WriteKind::SavedState]
    );
}

#[tokio::test]
async fn given_a_dropped_end_when_the_write_is_due_then_it_is_still_sent() {
    let rig = Rig::new();
    drop(rig.hand(answer()));
    rig.queue.drained().await;
    assert_eq!(
        rig.sender.kinds(),
        vec![WriteKind::Started, WriteKind::Answer]
    );
}

#[tokio::test]
async fn given_many_writes_when_handed_over_then_the_queue_has_no_cap_and_keeps_the_order() {
    let rig = Rig::new();
    let ends: Vec<_> = (0..2000u32).map(|i| rig.hand(step(i))).collect();
    for end in ends {
        assert_eq!(end.await, Ok(()));
    }
    let indexes: Vec<u32> = rig
        .sender
        .attempts_of(WriteKind::StepStarted)
        .iter()
        .map(|write| match write {
            crate::queue::Write::Fact(crate::backend::Fact::Step { index, .. }, _) => *index,
            _ => u32::MAX,
        })
        .collect();
    assert_eq!(indexes, (0..2000).collect::<Vec<_>>());
}
