use std::num::NonZeroUsize;

use br_agentic_contract::{IncomingItem, IncomingPort};
use br_llm_messages::IncomingEntry;
use serde_json::json;

use super::support::id;
use super::{World, notice_item, only_unread};
use crate::adapter::mark_gate_stored;
use br_runner_jobs_client::CallNoticeKind;
use br_runner_jobs_client::EntryKind;

fn text_of(item: &IncomingItem) -> String {
    let IncomingItem::Entry(entry) = item else {
        panic!("{item:?}");
    };
    let IncomingEntry::RuntimeNotice(notice) = entry.entry() else {
        panic!("{entry:?}");
    };
    notice.text().to_owned()
}

#[tokio::test]
async fn given_call_notices_when_they_are_listed_then_each_is_a_runtime_notice_whose_entry_id_is_the_item_id()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let kinds = [
        "ACCEPTED",
        "QUEUE_POSITION",
        "STARTED",
        "RUN_COMPLETED",
        "JOB_FAILED",
    ];
    let items = kinds
        .iter()
        .enumerate()
        .map(|(n, kind)| notice_item(agent.as_uuid(), n as u128 + 1, kind))
        .collect();
    world.jobs.graphql_default(only_unread(items));
    let context = world.context(agent);
    let batch = world
        .incoming()
        .items(&context, None, NonZeroUsize::new(10).expect("limit"))
        .await
        .expect("a batch");
    assert_eq!(batch.items.len(), 5);
    for (n, item) in batch.items.iter().enumerate() {
        let n = n as u128 + 1;
        assert_eq!(item.id().as_uuid(), id(100 + n));
        assert!(text_of(item).contains(&id(300 + n).to_string()));
        let entry = world
            .book
            .listed(agent.as_uuid(), id(100 + n))
            .expect("recorded");
        assert_eq!(entry.item_id, id(100 + n));
        assert_eq!(entry.child_job_id, Some(id(300 + n)));
    }
    let completed = world
        .book
        .listed(agent.as_uuid(), id(104))
        .expect("recorded");
    assert_eq!(
        completed.kind,
        EntryKind::CallNotice(CallNoticeKind::RunCompleted)
    );
}

#[tokio::test]
async fn given_a_completed_call_with_an_inline_result_when_it_is_listed_then_the_text_carries_the_result()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let mut item = notice_item(agent.as_uuid(), 1, "RUN_COMPLETED");
    item["answer"] = json!({
        "message": "the analysis is done",
        "attachments": [{ "name": "r.txt", "content_type": "text/plain", "content": "x" }],
    });
    world.jobs.graphql_default(only_unread(vec![item]));
    let context = world.context(agent);
    let batch = world
        .incoming()
        .items(&context, None, NonZeroUsize::new(10).expect("limit"))
        .await
        .expect("a batch");
    let text = text_of(&batch.items[0]);
    assert!(text.contains("the analysis is done"));
    assert!(text.contains("r.txt"));
}

async fn listed(
    world: &World,
    agent: br_llm_messages::AgentId,
    items: Vec<serde_json::Value>,
) -> Result<br_agentic_contract::IncomingBatch, br_agentic_contract::PortError> {
    world.jobs.graphql_default(only_unread(items));
    let context = world.context(agent);
    world
        .incoming()
        .items(&context, None, NonZeroUsize::new(20).expect("limit"))
        .await
}

#[tokio::test]
async fn given_every_kind_of_notice_when_it_is_listed_then_a_non_blank_runtime_notice_with_the_item_id_comes_back()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let kinds = [
        ("ACCEPTED", CallNoticeKind::Accepted),
        ("QUEUE_POSITION", CallNoticeKind::QueuePosition),
        ("STARTED", CallNoticeKind::RunStarted),
        ("RUN_COMPLETED", CallNoticeKind::RunCompleted),
        ("JOB_FAILED", CallNoticeKind::JobFailed),
        ("JOB_CANCELLED", CallNoticeKind::JobCancelled),
        ("REFUSED", CallNoticeKind::Refused),
    ];
    let items = kinds
        .iter()
        .enumerate()
        .map(|(n, (word, _))| {
            let mut item = notice_item(agent.as_uuid(), n as u128 + 1, word);
            item["message"] = json!("a plain word from jobs");
            item
        })
        .collect();
    let batch = listed(&world, agent, items).await.expect("a batch");
    for (n, (_, kind)) in kinds.iter().enumerate() {
        let n = n as u128 + 1;
        assert_eq!(batch.items[n as usize - 1].id().as_uuid(), id(100 + n));
        assert!(text_of(&batch.items[n as usize - 1]).contains("a plain word from jobs"));
        let entry = world
            .book
            .listed(agent.as_uuid(), id(100 + n))
            .expect("recorded");
        assert_eq!(entry.kind, EntryKind::CallNotice(*kind));
    }
}

#[tokio::test]
async fn given_a_mail_notice_when_it_is_listed_then_the_text_of_jobs_is_the_runtime_notice_with_the_item_id()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let item = json!({
        "itemId": id(160), "seq": 1, "itemKind": "MAIL_NOTICE", "address": agent.as_uuid(),
        "mailId": id(260), "message": "Your mail was refused: the address is gone.",
    });
    let batch = listed(&world, agent, vec![item]).await.expect("a batch");
    assert_eq!(batch.items[0].id().as_uuid(), id(160));
    assert_eq!(
        text_of(&batch.items[0]),
        "Your mail was refused: the address is gone."
    );
    let entry = world
        .book
        .listed(agent.as_uuid(), id(160))
        .expect("recorded");
    assert_eq!(entry.kind, EntryKind::MailNotice);
    assert_eq!(entry.child_job_id, None);
}

#[tokio::test]
async fn given_a_blank_mail_notice_or_an_item_id_that_is_not_a_uuid_v7_then_the_listing_is_an_error_of_the_port()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let blank = json!({
        "itemId": id(160), "seq": 1, "itemKind": "MAIL_NOTICE", "address": agent.as_uuid(),
        "mailId": null, "message": "  ",
    });
    assert!(listed(&world, agent, vec![blank]).await.is_err());
    let mut nil = notice_item(agent.as_uuid(), 1, "ACCEPTED");
    nil["itemId"] = json!(uuid::Uuid::nil());
    let error = listed(&world, agent, vec![nil]).await;
    assert!(error.is_err_and(|error| !error.is_refusal()));
}

#[tokio::test]
async fn given_a_notice_of_an_unknown_type_when_listed_then_the_listing_is_an_error_of_the_port_with_no_default_text()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let error = listed(
        &world,
        agent,
        vec![notice_item(agent.as_uuid(), 1, "PAUSED")],
    )
    .await
    .expect_err("an error of the port");
    assert!(!error.is_refusal());
    assert!(world.book.listed(agent.as_uuid(), id(101)).is_none());
}

#[tokio::test]
async fn given_an_item_of_an_unknown_kind_when_listed_then_the_listing_is_an_error_of_the_port_with_no_default_text()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let mut unknown_item = notice_item(agent.as_uuid(), 1, "ACCEPTED");
    unknown_item["itemKind"] = json!("ELSE");
    unknown_item["message"] = json!("Something new.");
    let error = listed(&world, agent, vec![unknown_item])
        .await
        .expect_err("an error of the port");
    assert!(!error.is_refusal());
    assert!(world.book.listed(agent.as_uuid(), id(101)).is_none());
}

#[tokio::test]
async fn given_a_malformed_item_for_an_address_when_it_is_listed_then_the_listing_of_that_address_is_an_error_of_the_port()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let mut bad = notice_item(agent.as_uuid(), 1, "RUN_COMPLETED");
    bad["resultId"] = json!("nope");
    world.jobs.graphql_default(only_unread(vec![bad]));
    let context = world.context(agent);
    let error = world
        .incoming()
        .items(&context, None, NonZeroUsize::new(10).expect("limit"))
        .await
        .expect_err("a fault");
    assert!(!error.is_refusal());
}
