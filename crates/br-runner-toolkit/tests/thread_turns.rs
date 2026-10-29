#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use br_agentic_contract::{
    InboxEntry, Incoming, MailId, MailItem, Stored, ThreadAccess, ThreadError, TurnTarget,
};
use br_llm_messages::{AgentId, IncomingEntry, InputTurn, RuntimeNotice, Turn};
use serde_json::Value;
use support::*;

fn mail_id() -> MailId {
    MailId::from_uuid(v7()).expect("a mail id")
}

fn stranger() -> AgentId {
    AgentId::from_uuid(v7()).expect("an agent id")
}

fn notice_entry(text: &str) -> IncomingEntry {
    IncomingEntry::from(RuntimeNotice::new(entry_id(), text).expect("a notice"))
}

#[tokio::test]
async fn given_a_mail_to_when_stored_as_an_input_turn_then_one_message_holds_every_value() {
    let rig = rig();
    let agent = rig.live_agent().await;
    rig.show(&rig.empty_thread(), agent);
    let (sender, copied) = (stranger(), stranger());
    let mail = Incoming::Mail(MailItem::To {
        id: mail_id(),
        sender,
        to: vec![agent],
        cc: vec![copied],
        subject: "Quarterly plan".to_owned(),
        body: "Write the draft by Friday".to_owned(),
        attachment_names: vec!["figures.xlsx".to_owned(), "notes.pdf".to_owned()],
    });
    let stored = rig
        .access
        .add_input_turn(&rig.thread, agent, turn_id(), vec![mail])
        .await
        .expect("stored");
    assert_eq!(stored, Stored::Added);
    let stores = rig.payloads("store");
    assert_eq!(stores.len(), 1);
    let text = stores[0].to_string();
    for expected in [
        "Quarterly plan",
        "Write the draft by Friday",
        "figures.xlsx",
        "notes.pdf",
        &agent.as_uuid().to_string(),
        &copied.as_uuid().to_string(),
        &sender.as_uuid().to_string(),
    ] {
        assert!(text.contains(expected), "{expected} is missing from {text}");
    }
}

#[tokio::test]
async fn given_a_cc_mail_when_stored_then_a_runtime_notice_holds_the_mail_id_and_the_subject() {
    let rig = rig();
    let agent = rig.live_agent().await;
    rig.show(&rig.empty_thread(), agent);
    let id = mail_id();
    let item = Incoming::Mail(MailItem::Cc {
        id,
        subject: "Budget review".to_owned(),
    });
    rig.access
        .add_input_turn(&rig.thread, agent, turn_id(), vec![item])
        .await
        .expect("stored");
    let stores = rig.payloads("store");
    let entry = stores[0]
        .pointer("/message/turn/entries/0")
        .expect("one entry")
        .clone();
    assert_eq!(entry["type"], "runtime_notice");
    assert_eq!(entry["id"], Value::String(id.to_string()));
    assert!(
        entry["text"]
            .as_str()
            .expect("text")
            .contains("Budget review")
    );
}

#[tokio::test]
async fn given_an_input_turn_when_stored_then_job_acting_user_and_impersonator_stay_empty() {
    let rig = rig();
    let agent = rig.live_agent().await;
    rig.show(&rig.empty_thread(), agent);
    let entry = InboxEntry::new(notice_entry("call notice")).expect("an inbox entry");
    rig.access
        .add_input_turn(&rig.thread, agent, turn_id(), vec![Incoming::Entry(entry)])
        .await
        .expect("stored");
    let store = &rig.payloads("store")[0];
    for key in ["job_id", "acting_user_id", "impersonator_id"] {
        assert!(store.get(key).is_none_or(Value::is_null), "{key} is set");
    }
    assert_eq!(store["message"]["author_name"], "Writer");
    assert_eq!(store["message"]["agent_id"], agent.as_uuid().to_string());
}

#[tokio::test]
async fn given_a_thread_with_turns_when_a_turn_is_added_then_it_follows_the_last_turn() {
    let rig = rig();
    let agent = rig.live_agent().await;
    let mut conversation = rig.empty_thread();
    let first = turn_id();
    conversation
        .push_turn(Turn::from(
            InputTurn::new(first, vec![notice_entry("earlier")]).expect("a turn"),
        ))
        .expect("pushed");
    rig.show(&conversation, agent);
    let entry = InboxEntry::new(notice_entry("later")).expect("an inbox entry");
    rig.access
        .add_input_turn(&rig.thread, agent, turn_id(), vec![Incoming::Entry(entry)])
        .await
        .expect("stored");
    assert_eq!(
        rig.payloads("store")[0]["parent_message_id"],
        Value::String(first.as_uuid().to_string())
    );
}

#[tokio::test]
async fn given_entries_the_thread_holds_when_stored_again_then_nothing_is_sent() {
    let rig = rig();
    let agent = rig.live_agent().await;
    let mut conversation = rig.empty_thread();
    let held = notice_entry("already there");
    conversation
        .push_turn(Turn::from(
            InputTurn::new(turn_id(), vec![held.clone()]).expect("a turn"),
        ))
        .expect("pushed");
    rig.show(&conversation, agent);
    let entry = InboxEntry::new(held).expect("an inbox entry");
    let stored = rig
        .access
        .add_input_turn(&rig.thread, agent, turn_id(), vec![Incoming::Entry(entry)])
        .await
        .expect("held");
    assert_eq!(stored, Stored::Held);
    assert!(rig.bus.commands().is_empty());
}

#[tokio::test]
async fn given_a_turn_identity_held_with_other_content_when_stored_then_the_addition_is_refused() {
    let rig = rig();
    let agent = rig.live_agent().await;
    let mut conversation = rig.empty_thread();
    let id = turn_id();
    conversation
        .push_turn(Turn::from(
            InputTurn::new(id, vec![notice_entry("first")]).expect("a turn"),
        ))
        .expect("pushed");
    rig.show(&conversation, agent);
    let entry = InboxEntry::new(notice_entry("second")).expect("an inbox entry");
    let error = rig
        .access
        .add_input_turn(&rig.thread, agent, id, vec![Incoming::Entry(entry)])
        .await
        .expect_err("refused");
    assert!(matches!(error, ThreadError::Refused(_)));
    assert!(rig.bus.commands().is_empty());
}

#[tokio::test]
async fn given_an_input_turn_target_when_an_entry_is_added_then_the_addition_is_refused() {
    let rig = rig();
    let agent = rig.live_agent().await;
    rig.show(&rig.empty_thread(), agent);
    let entry = InboxEntry::new(notice_entry("late")).expect("an inbox entry");
    let error = rig
        .access
        .add_incoming(
            &rig.thread,
            agent,
            TurnTarget::Input(turn_id()),
            Incoming::Entry(entry),
        )
        .await
        .expect_err("refused");
    assert!(matches!(error, ThreadError::Refused(_)));
}

#[tokio::test]
async fn given_a_missing_thread_when_loaded_then_a_port_error() {
    let rig = rig();
    let agent = rig.live_agent().await;
    rig.graphql
        .reply_with(Ok(br_runner_threads::transport::HttpReply {
            status: 200,
            body: br#"{"data":{"threadsContext":null}}"#.to_vec(),
        }));
    let error = rig
        .access
        .load(&rig.thread, agent)
        .await
        .expect_err("missing");
    let port = error
        .cause()
        .downcast_ref::<br_runner_toolkit::PortError>()
        .expect("a port error");
    assert_eq!(port.code(), br_runner_toolkit::PortErrorCode::ThreadsFailed);
}
