use super::support::*;
use std::time::Duration;

use crate::error::{Error, Operation};
use crate::queue::{QueueReport, Removal, Write, WriteKind};

fn resendable_errors() -> Vec<Error> {
    vec![
        unavailable(),
        lost(),
        Error::wait_fired("late"),
        unreadable(),
        fetch_failed(),
    ]
}

fn final_errors() -> Vec<Error> {
    vec![
        Error::Unauthenticated,
        no_usable(),
        Error::invalid("bad"),
        Error::InfraMissing("stream".to_owned()),
        Error::BookDropped,
    ]
}

fn independent_writes() -> Vec<Write> {
    vec![
        answer(),
        saved(),
        plan(),
        step(3),
        completed(),
        failed_fact(),
        upload(40),
        create(41, 42),
        registration(10),
        mail(20, 99, 98),
        finish(43, 44),
    ]
}

#[tokio::test]
async fn given_a_resendable_failure_when_a_write_is_sent_then_it_is_sent_again_identical_and_each_failed_attempt_is_reported()
 {
    for write in independent_writes() {
        for error in resendable_errors() {
            let sender = ScriptedSender::new();
            sender.script_write(&write, vec![fail(error.clone()); 3]);
            let rig = Rig::with(sender);
            let end = rig.hand(write.clone());
            assert_eq!(end.await, Ok(()), "{write:?} after {error:?}");
            let sent = rig.sender.attempts_of(write.name().kind);
            assert_eq!(sent.len(), 4);
            assert!(sent.iter().all(|attempt| *attempt == write));
            let retrying = rig.observer.retrying();
            assert_eq!(retrying.len(), 3);
            assert!(
                retrying
                    .iter()
                    .all(|(name, e)| *name == write.name() && *e == error)
            );
            assert_eq!(rig.observer.removed(), Vec::new());
            assert!(rig.observer.delivered().contains(&write.name()));
        }
    }
}

#[tokio::test]
async fn given_the_started_fact_when_jobs_refuses_it_as_instance_unknown_then_it_is_sent_again_with_the_same_ids()
 {
    let sender = ScriptedSender::new();
    sender.script(
        WriteKind::Started,
        vec![fail(refusal(Operation::StartedFact, "instance_unknown")); 2],
    );
    let rig = Rig::with(sender);
    assert_eq!(rig.started.await, Ok(()));
    let sent = rig.sender.attempts_of(WriteKind::Started);
    assert_eq!(sent.len(), 3);
    assert!(sent.iter().all(|attempt| *attempt == sent[0]));
    assert_eq!(rig.observer.retrying().len(), 2);
}

#[tokio::test]
async fn given_the_started_fact_when_jobs_refuses_it_for_another_reason_then_it_is_final_and_removed_as_refused()
 {
    for reason in [
        "run_not_started",
        "start_id_conflict",
        "run_not_pending",
        "unknown_reason",
    ] {
        let error = refusal(Operation::StartedFact, reason);
        let sender = ScriptedSender::new();
        sender.script(WriteKind::Started, vec![fail(error.clone())]);
        let rig = Rig::with(sender);
        assert_eq!(rig.started.await, Err(Removal::Refused(error)));
        assert_eq!(rig.sender.attempts().len(), 1);
        assert!(rig.observer.retrying().is_empty());
    }
}

#[tokio::test]
async fn given_a_refusal_instance_unknown_on_another_write_when_sent_then_it_is_final() {
    for write in independent_writes() {
        let error = refusal(Operation::Mail, "instance_unknown");
        let sender = ScriptedSender::new();
        sender.script_write(&write, vec![fail(error.clone())]);
        let rig = Rig::with(sender);
        assert_eq!(rig.hand(write.clone()).await, Err(Removal::Refused(error)));
        assert_eq!(rig.sender.attempts_of(write.name().kind).len(), 1);
    }
}

#[tokio::test]
async fn given_a_final_failure_when_a_write_is_sent_then_it_is_removed_as_failed_with_that_error_after_one_attempt()
 {
    for write in independent_writes() {
        for error in final_errors() {
            let sender = ScriptedSender::new();
            sender.script_write(&write, vec![fail(error.clone())]);
            let rig = Rig::with(sender);
            let end = rig.hand(write.clone());
            assert_eq!(end.await, Err(Removal::Failed(error.clone())));
            assert_eq!(rig.sender.attempts_of(write.name().kind).len(), 1);
            assert!(rig.observer.retrying().is_empty());
            assert_eq!(
                rig.observer.removed(),
                vec![(write.name(), Removal::Failed(error))]
            );
        }
    }
}

#[tokio::test]
async fn given_a_refusal_of_jobs_when_a_write_is_sent_then_it_is_removed_as_refused_and_the_caller_gets_that_refusal()
 {
    for write in independent_writes() {
        let error = refusal(Operation::Registration, "some_reason");
        let sender = ScriptedSender::new();
        sender.script_write(&write, vec![fail(error.clone())]);
        let rig = Rig::with(sender);
        let removal = rig.hand(write.clone()).await.expect_err("removed");
        assert_eq!(removal, Removal::Refused(error.clone()));
        assert_eq!(removal.into_error(), Some(error));
    }
}

#[tokio::test]
async fn given_a_resend_that_jobs_refuses_when_it_arrives_then_the_refusal_is_final() {
    let sender = ScriptedSender::new();
    let error = refusal(Operation::Mail, "conflicting_id");
    sender.script_write(&mail(20, 99, 98), vec![fail(lost()), fail(error.clone())]);
    let rig = Rig::with(sender);
    let end = rig.hand(mail(20, 99, 98));
    assert_eq!(end.await, Err(Removal::Refused(error)));
    assert_eq!(rig.sender.attempts_of(WriteKind::Mail).len(), 2);
    assert_eq!(rig.observer.retrying().len(), 1);
}

#[tokio::test]
async fn given_a_final_failure_when_later_writes_wait_then_they_go_on_and_are_delivered() {
    let sender = ScriptedSender::new();
    sender.script(WriteKind::Answer, vec![fail(Error::Unauthenticated)]);
    let rig = Rig::with(sender);
    let first = rig.hand(answer());
    let second = rig.hand(saved());
    assert_eq!(first.await, Err(Removal::Failed(Error::Unauthenticated)));
    assert_eq!(second.await, Ok(()));
}

#[tokio::test]
async fn given_a_retrying_write_when_its_reports_are_read_then_each_names_the_same_write_and_none_is_a_removal()
 {
    let sender = ScriptedSender::new();
    sender.script(
        WriteKind::SavedState,
        vec![fail(unavailable()), fail(lost())],
    );
    let rig = Rig::with(sender);
    assert_eq!(rig.hand(saved()).await, Ok(()));
    let kinds: Vec<&str> = rig
        .observer
        .reports()
        .iter()
        .map(|report| match report {
            QueueReport::Retrying { .. } => "retrying",
            QueueReport::Removed { .. } => "removed",
            QueueReport::Delivered { .. } => "delivered",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["delivered", "retrying", "retrying", "delivered"]
    );
}

#[tokio::test]
async fn given_a_write_sent_in_several_attempts_then_one_session_serves_all_of_them_and_a_hung_attempt_does_not_end_it()
 {
    let sender = ScriptedSender::new();
    let write = create(41, 42);
    sender.script_write(&write, vec![Step::Hang, fail(lost()), fail(lost())]);
    let rig = Rig::timed(sender, Duration::from_millis(30), Duration::from_millis(1));
    assert_eq!(rig.hand(write.clone()).await, Ok(()));
    assert_eq!(rig.sender.attempts_of(WriteKind::Create).len(), 4);
    let other = finish(43, 44);
    assert_eq!(rig.hand(other).await, Ok(()));
    assert_eq!(rig.sender.sessions(), 3);
}
