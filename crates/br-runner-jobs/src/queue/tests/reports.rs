use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::support::*;
use crate::error::{Error, Operation};
use crate::queue::{
    DeliveryQueue, QueueObserver, QueueReport, QueueWriter, Removal, Write, WriteKind,
};

fn leaks(text: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    for (label, needle) in [
        ("passport", PASSPORT_TEXT.to_owned()),
        ("body", BODY_TEXT.to_owned()),
        ("parent", id(PARENT).to_string()),
        ("parent", id(PARENT).simple().to_string()),
        ("initial", id(INITIAL).to_string()),
        ("initial", id(INITIAL).simple().to_string()),
        ("work job", id(WORK_JOB).to_string()),
    ] {
        if text.contains(&needle) {
            found.push(label);
        }
    }
    found
}

#[test]
fn given_a_create_with_secrets_when_it_is_formatted_whole_then_the_check_for_leaks_sees_them() {
    let text = format!("{:?}", create(41, 42));
    assert_eq!(leaks(&text), vec!["passport", "parent", "initial"]);
}

#[test]
fn given_every_kind_of_write_when_it_is_named_then_the_name_holds_the_kind_and_the_ids_only() {
    let cases = vec![
        (create(41, 42), WriteKind::Create, vec![id(41), id(42)]),
        (finish(41, 43), WriteKind::Finish, vec![id(41), id(43)]),
        (cancel(41, 44), WriteKind::Cancel, vec![id(41), id(44)]),
        (upload(41), WriteKind::Upload, vec![id(41)]),
        (registration(10), WriteKind::Registration, vec![id(10)]),
        (mail(20, 10, 98), WriteKind::Mail, vec![id(20), id(10)]),
        (marker(30, 10), WriteKind::ReadMarker, vec![id(30), id(10)]),
        (answer(), WriteKind::Answer, vec![]),
        (saved(), WriteKind::SavedState, vec![]),
        (plan(), WriteKind::PlanDeclared, vec![id(RUN)]),
        (step(1), WriteKind::StepStarted, vec![id(RUN)]),
        (completed(), WriteKind::Completed, vec![id(RUN)]),
        (failed_fact(), WriteKind::Failed, vec![id(RUN)]),
        (
            Write::Started {
                run_id: id(RUN),
                start_id: id(START),
            },
            WriteKind::Started,
            vec![id(RUN), id(START)],
        ),
    ];
    for (write, kind, ids) in cases {
        let name = write.name();
        assert_eq!((name.kind, name.ids.clone()), (kind, ids));
        assert_eq!(leaks(&format!("{name:?}")), Vec::<&str>::new());
    }
}

#[tokio::test]
async fn given_a_busy_run_when_the_reports_and_the_state_are_formatted_then_no_token_passport_job_or_content_appears()
 {
    let sender = ScriptedSender::new();
    sender.script(
        WriteKind::Create,
        vec![fail(unavailable()), fail(fetch_failed())],
    );
    sender.script(WriteKind::Answer, vec![fail(lost())]);
    sender.script(
        WriteKind::Mail,
        vec![fail(refusal(Operation::Mail, "no_recipient"))],
    );
    sender.always(WriteKind::SavedState, Step::Hang);
    let rig = Rig::with(sender);
    let ends = vec![
        rig.hand(upload(40)),
        rig.hand(create(41, 42)),
        rig.hand(finish(41, 43)),
        rig.hand(answer()),
        rig.hand(mail(20, 10, 98)),
        rig.hand(registration(10)),
        rig.hand(plan()),
    ];
    for end in ends {
        let _ = end.await;
    }
    let _stuck = rig.hand(saved());
    let (_gate, _held) = rig.hold(marker(30, 10), None);
    settle().await;
    let reports = format!("{:?}", rig.observer.reports());
    let state = format!("{:?}", rig.queue.state());
    assert_eq!(leaks(&reports), Vec::<&str>::new());
    assert_eq!(leaks(&state), Vec::<&str>::new());
    assert!(reports.contains("Retrying") && reports.contains("Delivered"));
}

#[tokio::test]
async fn given_a_refused_mail_when_the_removal_is_reported_then_the_report_carries_the_text_of_jobs()
 {
    let sender = ScriptedSender::new();
    sender.script(
        WriteKind::Mail,
        vec![fail(Error::refused(
            Operation::Mail,
            "unknown_recipient",
            "agent 7 is not here, ask the host",
        ))],
    );
    let rig = Rig::with(sender);
    assert!(rig.hand(mail(20, 10, 98)).await.is_err());
    let removed = rig.observer.removed();
    assert_eq!(removed.len(), 1);
    match &removed[0].1 {
        Removal::Refused(Error::Refused(refusal)) => {
            assert_eq!(refusal.message, "agent 7 is not here, ask the host");
            assert_eq!(refusal.reason, "unknown_recipient");
        }
        other => panic!("not a refusal: {other:?}"),
    }
}

#[tokio::test]
async fn given_a_mix_of_endings_when_the_run_is_drained_then_every_write_has_exactly_one_report_of_its_end()
 {
    let sender = ScriptedSender::new();
    sender.script(WriteKind::Answer, vec![fail(Error::Unauthenticated)]);
    sender.script(
        WriteKind::Upload,
        vec![fail(refusal(Operation::Upload, "size"))],
    );
    let rig = Rig::with(sender);
    let writes = vec![
        answer(),
        saved(),
        completed(),
        upload(41),
        create(41, 42),
        finish(41, 43),
        plan(),
        mail(20, 99, 98),
    ];
    for write in &writes {
        drop(rig.hand(write.clone()));
    }
    let (gate, _end) = rig.hold(marker(30, 10), None);
    gate.send(crate::queue::Gate::Dropped).expect("awaited");
    rig.queue.drained().await;
    let mut ended: Vec<_> = rig.observer.delivered();
    ended.extend(rig.observer.removed().into_iter().map(|(name, _)| name));
    let mut expected: Vec<_> = writes.iter().map(Write::name).collect();
    expected.push(marker(30, 10).name());
    expected.push(
        Write::Started {
            run_id: id(RUN),
            start_id: id(START),
        }
        .name(),
    );
    let key = |name: &crate::queue::WriteName| format!("{:?}", name);
    ended.sort_by_key(key);
    expected.sort_by_key(key);
    assert_eq!(ended, expected);
}

struct Reentrant {
    writer: Mutex<Option<QueueWriter>>,
    snapshots: Mutex<usize>,
}

impl QueueObserver for Reentrant {
    fn report(&self, report: &QueueReport) {
        let writer = self.writer.lock().unwrap().clone();
        if let Some(writer) = writer {
            let _ = writer.shared.snapshot();
            *self.snapshots.lock().unwrap() += 1;
            if matches!(report, QueueReport::Delivered { write } if write.kind == WriteKind::Answer)
            {
                drop(writer.hand_over(plan()));
            }
        }
    }
}

#[tokio::test]
async fn given_an_observer_that_calls_the_queue_when_it_gets_a_report_then_the_report_does_not_hold_delivery()
 {
    let observer = Arc::new(Reentrant {
        writer: Mutex::new(None),
        snapshots: Mutex::new(0),
    });
    let sender = ScriptedSender::new();
    let settings = settings_with(
        observer.clone(),
        Duration::from_secs(60),
        Duration::from_millis(1),
    );
    let (queue, _) = DeliveryQueue::start(sender.clone(), settings, id(RUN)).expect("a queue");
    *observer.writer.lock().unwrap() = Some(queue.writer());
    let end = queue.answer(content("x"));
    let state = tokio::time::timeout(Duration::from_secs(10), async {
        assert_eq!(end.await, Ok(()));
        queue.drained().await
    })
    .await
    .expect("no deadlock");
    let _ = state;
    sender.attempts_reached(3).await;
    assert!(sender.kinds().contains(&WriteKind::PlanDeclared));
    assert!(*observer.snapshots.lock().unwrap() >= 2);
}
