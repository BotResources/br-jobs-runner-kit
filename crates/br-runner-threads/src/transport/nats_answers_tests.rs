use std::time::Duration;

use serde_json::{Value, json};
use uuid::Uuid;

use super::*;

#[path = "../../tests/support/server.rs"]
mod server;

const THREAD_CONFIRMED: &str = "integration.evt.threads.thread.confirmed.v1";
const THREAD_REFUSED: &str = "integration.evt.threads.thread.refused.v1";
const MESSAGE_CONFIRMED: &str = "integration.evt.threads.message.confirmed.v1";
const MESSAGE_REFUSED: &str = "integration.evt.threads.message.refused.v1";

fn bytes(payload: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({ "payload": payload })).expect("json")
}

fn thread_payload(id: Uuid) -> Value {
    json!({ "command_id": id, "command_type": "thread.create", "thread_id": Uuid::now_v7() })
}

fn message_payload(id: Uuid) -> Value {
    json!({
        "command_id": id,
        "command_type": "message.store",
        "thread_id": Uuid::now_v7(),
        "message_id": Uuid::now_v7(),
        "status": "COMPLETED",
        "deleted": false,
    })
}

fn refused(mut payload: Value) -> Value {
    payload["reason_code"] = json!("ARTIFACT_NOT_READY");
    payload["params"] = json!({ "artifact_id": "a" });
    payload
}

#[test]
fn the_four_subjects_are_the_contract_answers_of_threads() {
    let subjects: Vec<String> = routes()
        .iter()
        .map(|(subject, _)| subject.clone())
        .collect();
    assert_eq!(
        subjects,
        [
            THREAD_CONFIRMED,
            THREAD_REFUSED,
            MESSAGE_CONFIRMED,
            MESSAGE_REFUSED
        ]
    );
}

#[test]
fn each_subject_is_read_as_its_own_contract_event() {
    let id = Uuid::now_v7();
    let (found, answer) = read_answer(THREAD_CONFIRMED, &bytes(thread_payload(id))).expect("read");
    assert_eq!(found, id);
    assert_eq!(
        answer,
        Ok(Answer::Confirmed(Confirmed {
            command_id: id,
            status: None
        }))
    );
    let (_, answer) = read_answer(MESSAGE_CONFIRMED, &bytes(message_payload(id))).expect("read");
    assert_eq!(
        answer,
        Ok(Answer::Confirmed(Confirmed {
            command_id: id,
            status: Some(MessageStatus::Completed)
        }))
    );
    for (subject, payload) in [
        (THREAD_REFUSED, refused(thread_payload(id))),
        (MESSAGE_REFUSED, refused(message_payload(id))),
    ] {
        match read_answer(subject, &bytes(payload)).expect("read").1 {
            Ok(Answer::Refused(refusal)) => {
                assert_eq!(refusal.code, "ARTIFACT_NOT_READY");
                assert_eq!(refusal.params["artifact_id"], "a");
                assert_eq!(refusal.command_id, Some(id));
            }
            other => panic!("refused, not {other:?}"),
        }
    }
}

#[test]
fn nothing_but_the_four_subjects_is_an_answer() {
    let id = Uuid::now_v7();
    for subject in [
        "integration.evt.threads.thread.deleted.v1",
        "integration.evt.other.thread.refused.v1",
        "integration.evt.other.thread.confirmed.v1",
        "integration.evt.threads.thread.confirmed.v2",
        "integration.evt.threads.thread.refused.v1.extra",
        "",
    ] {
        assert!(
            read_answer(subject, &bytes(thread_payload(id))).is_none(),
            "{subject}"
        );
    }
}

#[test]
fn an_answer_that_does_not_fit_its_contract_event_is_unreadable_for_the_command_it_names() {
    let id = Uuid::now_v7();
    let mut without_status = message_payload(id);
    without_status
        .as_object_mut()
        .expect("object")
        .remove("status");
    let mut unknown_status = message_payload(id);
    unknown_status["status"] = json!("NOT_A_STATUS");
    let mut unknown_reason = refused(thread_payload(id));
    unknown_reason["reason_code"] = json!("NOT_A_REASON");
    let mut thread_without_id = thread_payload(id);
    thread_without_id
        .as_object_mut()
        .expect("object")
        .remove("thread_id");
    for (subject, payload) in [
        (MESSAGE_CONFIRMED, without_status),
        (MESSAGE_CONFIRMED, unknown_status),
        (THREAD_REFUSED, unknown_reason),
        (THREAD_CONFIRMED, thread_without_id),
    ] {
        let (found, answer) = read_answer(subject, &bytes(payload)).expect("read");
        assert_eq!(found, id);
        assert!(matches!(answer, Err(Error::Unreadable(_))), "{subject}");
    }
}

#[test]
fn a_frame_without_a_readable_command_id_is_dropped() {
    assert!(read_answer(THREAD_CONFIRMED, b"not json").is_none());
    assert!(read_answer(THREAD_CONFIRMED, &bytes(json!({}))).is_none());
    assert!(read_answer(THREAD_CONFIRMED, &bytes(json!({ "command_id": "nope" }))).is_none());
}

#[tokio::test]
#[ignore = "needs a nats-server binary"]
async fn the_next_command_reopens_the_subscription_after_it_closed() {
    let server = server::Server::start();
    let client = server.client().await;
    let answers = Answers::new(client.clone());
    answers.ensure_open().await.expect("open");

    let first = answers.register(Uuid::now_v7()).expect("registered");
    answers.closed();
    assert!(matches!(first.into_wait().await, Err(Error::AnswerLost(_))));
    assert!(!locked(&answers.state).open);

    answers.ensure_open().await.expect("reopened");
    let id = Uuid::now_v7();
    let waiter = answers.register(id).expect("registered");
    client
        .publish(THREAD_CONFIRMED, bytes(thread_payload(id)).into())
        .await
        .expect("publish");
    client.flush().await.expect("flush");
    let answer = tokio::time::timeout(Duration::from_secs(5), waiter.into_wait())
        .await
        .expect("answered");
    assert_eq!(
        answer,
        Ok(Answer::Confirmed(Confirmed {
            command_id: id,
            status: None
        }))
    );
}
