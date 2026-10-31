use super::support::*;
use crate::error::{Error, Operation};
use crate::queue::{Gate, Removal, Write, WriteEnd, WriteKind};

fn refused(operation: Operation) -> Error {
    refusal(operation, "refused_by_jobs")
}

fn everything_but_started() -> Vec<Write> {
    vec![
        answer(),
        saved(),
        plan(),
        step(1),
        completed(),
        failed_fact(),
        upload(40),
        create(41, 42),
        finish(43, 44),
        cancel(45, 46),
        registration(10),
        mail(20, 99, 98),
    ]
}

fn removed_for(error: Error) -> Removal {
    match error {
        Error::Refused(_) => Removal::Refused(error),
        other => Removal::Failed(other),
    }
}

#[tokio::test]
async fn given_a_removed_started_fact_when_any_write_waits_then_it_is_removed_as_depending_on_it_and_never_sent()
 {
    for cause in [
        refused(Operation::StartedFact),
        Error::Unauthenticated,
        no_usable(),
    ] {
        let sender = ScriptedSender::new();
        let latch = Latch::new();
        sender.script(
            WriteKind::Started,
            vec![Step::After(latch.clone(), Err(cause.clone()))],
        );
        let rig = Rig::with(sender);
        let mut ends: Vec<WriteEnd> = everything_but_started()
            .into_iter()
            .map(|w| rig.hand(w))
            .collect();
        let (gate, marker_end) = rig.hold(marker(30, 10), None);
        gate.send(Gate::Stored).expect("the task awaits the gate");
        ends.push(marker_end);
        settle().await;
        latch.open();
        assert_eq!(rig.started.await, Err(removed_for(cause)));
        for end in ends {
            assert_eq!(end.await, Err(Removal::DependsOnRemoved));
        }
        assert_eq!(rig.sender.kinds(), vec![WriteKind::Started]);
    }
}

#[tokio::test]
async fn given_a_removed_started_fact_when_a_write_is_handed_over_later_then_it_is_removed_at_once()
{
    let sender = ScriptedSender::new();
    sender.script(
        WriteKind::Started,
        vec![fail(refused(Operation::StartedFact))],
    );
    let rig = Rig::with(sender);
    let mut rig = rig;
    let _ = (&mut rig.started).await;
    for write in everything_but_started() {
        let mut end = rig.hand(write.clone());
        assert_eq!(
            ready(&mut end),
            Some(Err(Removal::DependsOnRemoved)),
            "{write:?}"
        );
    }
    assert_eq!(rig.sender.kinds(), vec![WriteKind::Started]);
}

#[tokio::test]
async fn given_a_removed_registration_when_its_agent_sends_mail_then_the_mail_depends_on_it_and_other_agents_are_not_touched()
 {
    for cause in [refused(Operation::Registration), Error::Unauthenticated] {
        let sender = ScriptedSender::new();
        sender.script_write(&registration(10), vec![fail(cause.clone())]);
        let rig = Rig::with(sender);
        let reg_10 = rig.hand(registration(10));
        let reg_11 = rig.hand(registration(11));
        let queued = rig.hand(mail(20, 10, 99));
        let other = rig.hand(mail(21, 11, 99));
        let no_registration = rig.hand(mail(22, 12, 99));
        assert_eq!(reg_10.await, Err(removed_for(cause)));
        assert_eq!(reg_11.await, Ok(()));
        assert_eq!(queued.await, Err(Removal::DependsOnRemoved));
        assert_eq!(other.await, Ok(()));
        assert_eq!(no_registration.await, Ok(()));
        let mut late = rig.hand(mail(23, 10, 99));
        assert_eq!(ready(&mut late), Some(Err(Removal::DependsOnRemoved)));
        let sent_mails: Vec<_> = rig.sender.attempts_of(WriteKind::Mail);
        assert_eq!(sent_mails, vec![mail(21, 11, 99), mail(22, 12, 99)]);
    }
}

#[tokio::test]
async fn given_a_removed_registration_when_its_agent_makes_a_background_create_then_the_create_depends_on_it_and_a_sync_create_or_another_agent_is_not_touched()
 {
    let sender = ScriptedSender::new();
    sender.script_write(
        &registration(10),
        vec![fail(refused(Operation::Registration))],
    );
    let rig = Rig::with(sender);
    let reg = rig.hand(registration(10));
    let caller_10 = rig.hand(background_create(41, 42, 10));
    let caller_11 = rig.hand(background_create(43, 44, 11));
    let sync = rig.hand(create(45, 46));
    assert!(reg.await.is_err());
    assert_eq!(caller_10.await, Err(Removal::DependsOnRemoved));
    assert_eq!(caller_11.await, Ok(()));
    assert_eq!(sync.await, Ok(()));
    let mut late = rig.hand(background_create(47, 48, 10));
    assert_eq!(ready(&mut late), Some(Err(Removal::DependsOnRemoved)));
}

#[tokio::test]
async fn given_a_delivered_registration_when_its_agent_sends_mail_then_the_mail_is_sent_after_it() {
    let rig = Rig::new();
    let reg = rig.hand(registration(10));
    let sent = rig.hand(mail(20, 10, 99));
    assert_eq!(reg.await, Ok(()));
    assert_eq!(sent.await, Ok(()));
    assert_eq!(
        rig.sender.kinds(),
        vec![WriteKind::Started, WriteKind::Registration, WriteKind::Mail]
    );
}

#[tokio::test]
async fn given_a_removed_registration_when_its_marker_is_released_then_the_marker_is_removed_as_depending_on_it()
 {
    let sender = ScriptedSender::new();
    sender.script_write(
        &registration(10),
        vec![fail(refused(Operation::Registration))],
    );
    let rig = Rig::with(sender);
    let reg = rig.hand(registration(10));
    let (gate_10, end_10) = rig.hold(marker(30, 10), None);
    let (gate_11, end_11) = rig.hold(marker(31, 11), None);
    assert!(reg.await.is_err());
    gate_10.send(Gate::Stored).expect("awaited");
    gate_11.send(Gate::Stored).expect("awaited");
    assert_eq!(end_10.await, Err(Removal::DependsOnRemoved));
    assert_eq!(end_11.await, Ok(()));
    assert_eq!(
        rig.sender.attempts_of(WriteKind::ReadMarker),
        vec![marker(31, 11)]
    );
}

#[tokio::test]
async fn given_a_removed_create_when_its_finish_and_cancel_wait_then_they_depend_on_it_and_other_jobs_are_not_touched()
 {
    for cause in [refused(Operation::Create), Error::Unauthenticated] {
        let sender = ScriptedSender::new();
        sender.script_write(&create(41, 42), vec![fail(cause.clone())]);
        let rig = Rig::with(sender);
        let bad = rig.hand(create(41, 42));
        let good = rig.hand(create(51, 52));
        let finish_bad = rig.hand(finish(41, 43));
        let cancel_bad = rig.hand(cancel(41, 44));
        let finish_good = rig.hand(finish(51, 53));
        let cancel_good = rig.hand(cancel(51, 54));
        let finish_unknown = rig.hand(finish(61, 55));
        assert_eq!(bad.await, Err(removed_for(cause)));
        assert_eq!(good.await, Ok(()));
        assert_eq!(finish_bad.await, Err(Removal::DependsOnRemoved));
        assert_eq!(cancel_bad.await, Err(Removal::DependsOnRemoved));
        assert_eq!(finish_good.await, Ok(()));
        assert_eq!(cancel_good.await, Ok(()));
        assert_eq!(finish_unknown.await, Ok(()));
        let mut late = rig.hand(cancel(41, 56));
        assert_eq!(ready(&mut late), Some(Err(Removal::DependsOnRemoved)));
    }
}
