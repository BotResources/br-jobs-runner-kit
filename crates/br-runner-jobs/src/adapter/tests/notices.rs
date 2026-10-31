use std::num::NonZeroUsize;

use br_agentic_contract::{InboxItem, InboxPort};
use br_llm_messages::IncomingEntry;
use serde_json::json;

use super::{World, notice_item, only_unread};
use crate::adapter::inbox_port::convert::convert;
use crate::adapter::mark_gate_stored;
use crate::book::EntryKind;
use crate::book::tests::id;
use crate::model::{CallNotice, CallNoticeKind, UnreadItem};

fn text_of(item: &InboxItem) -> String {
    let InboxItem::Entry(entry) = item else {
        panic!("{item:?}");
    };
    let IncomingEntry::RuntimeNotice(notice) = entry.entry() else {
        panic!("{entry:?}");
    };
    notice.text().to_owned()
}

fn notice(kind: CallNoticeKind) -> CallNotice {
    CallNotice {
        item_id: id(150),
        address: Some(id(70)),
        kind,
        child_job_id: Some(id(350)),
        position: None,
        message: None,
        report: None,
        inline: None,
        result_ref: None,
    }
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
        .inbox()
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
        .inbox()
        .items(&context, None, NonZeroUsize::new(10).expect("limit"))
        .await
        .expect("a batch");
    let text = text_of(&batch.items[0]);
    assert!(text.contains("the analysis is done"));
    assert!(text.contains("r.txt"));
}

#[tokio::test]
async fn given_every_kind_of_notice_when_it_is_converted_then_a_non_blank_runtime_notice_with_the_item_id_comes_back()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let kinds = [
        CallNoticeKind::Accepted,
        CallNoticeKind::QueuePosition,
        CallNoticeKind::RunStarted,
        CallNoticeKind::RunCompleted,
        CallNoticeKind::JobFailed,
        CallNoticeKind::JobCancelled,
        CallNoticeKind::Refused,
    ];
    for kind in kinds {
        let mut call = notice(kind);
        call.message = Some("a plain word from jobs".to_owned());
        let done = convert(&world.book, agent.as_uuid(), UnreadItem::CallNotice(call))
            .await
            .expect("converted");
        assert_eq!(done.item.id().as_uuid(), id(150));
        assert!(text_of(&done.item).contains("a plain word from jobs"));
        assert_eq!(done.entry.kind, EntryKind::CallNotice(kind));
    }
}

#[tokio::test]
async fn given_a_mail_notice_when_it_is_converted_then_the_text_of_jobs_is_the_runtime_notice_with_the_item_id()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let unread = UnreadItem::MailNotice {
        item_id: id(160),
        address: Some(agent.as_uuid()),
        mail_id: Some(id(260)),
        text: "Your mail was refused: the address is gone.".to_owned(),
    };
    let done = convert(&world.book, agent.as_uuid(), unread)
        .await
        .expect("converted");
    assert_eq!(done.item.id().as_uuid(), id(160));
    assert_eq!(
        text_of(&done.item),
        "Your mail was refused: the address is gone."
    );
    assert_eq!(done.entry.kind, EntryKind::MailNotice);
    assert_eq!(done.entry.child_job_id, None);
}

#[tokio::test]
async fn given_a_blank_mail_notice_or_an_item_id_that_is_not_a_uuid_v7_then_the_conversion_is_an_error_of_the_port()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let blank = UnreadItem::MailNotice {
        item_id: id(160),
        address: None,
        mail_id: None,
        text: "  ".to_owned(),
    };
    assert!(convert(&world.book, agent.as_uuid(), blank).await.is_err());
    let mut call = notice(CallNoticeKind::Accepted);
    call.item_id = uuid::Uuid::nil();
    let error = convert(&world.book, agent.as_uuid(), UnreadItem::CallNotice(call)).await;
    assert!(error.is_err_and(|error| !error.is_refusal()));
}
