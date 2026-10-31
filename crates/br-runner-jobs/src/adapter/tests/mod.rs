mod agent_port;
mod documents;
mod gates;
mod listing;
mod marking;
mod notices;
mod sending;
mod waiting;
mod world;

pub(super) use world::{ManualGate, Snap, World, instance, label, thread};

use std::time::Duration;

use contract_jobs::graphql_roots::*;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::book::tests::{everything_succeeds, id};
use crate::queue::DeliveryQueue;
use crate::test_double::{Call, ScriptedJobs};

pub(super) fn wire(items: Vec<Value>, mails: Value, attachments: Value) -> Value {
    let mut all = everything_succeeds();
    if let Some(root) = all.as_object_mut() {
        root.insert(
            UNREAD_ROOT.to_owned(),
            json!({ "success": true, "reason": null, "message": null, "items": items }),
        );
        root.insert(MAIL_ROOT.to_owned(), mails);
        root.insert(ATTACHMENT_ROOT.to_owned(), attachments);
    }
    all
}

pub(super) fn only_unread(items: Vec<Value>) -> Value {
    wire(items, refused("mail_not_yours"), refused("mail_not_yours"))
}

pub(super) fn refused(reason: &str) -> Value {
    json!({ "success": false, "reason": reason, "message": "The mail was not sent to this address." })
}

pub(super) fn mail_item(agent: Uuid, n: u128) -> Value {
    json!({
        "itemId": id(100 + n), "seq": n, "itemKind": "MAIL", "address": agent, "runId": id(2),
        "mailId": id(200 + n), "subject": "subject", "from": id(5), "author": "Ann",
        "sourceConversationId": id(6), "body": "body",
        "attachments": [{ "attachmentId": id(7), "name": "f.txt", "contentType": "text/plain", "size": 3 }],
    })
}

pub(super) fn cc_item(agent: Uuid, n: u128) -> Value {
    json!({
        "itemId": id(100 + n), "seq": n, "itemKind": "CC_NOTICE", "address": agent,
        "mailId": id(200 + n), "subject": "cc subject", "attachments": null, "noticeType": null,
    })
}

pub(super) fn notice_item(agent: Uuid, n: u128, notice_type: &str) -> Value {
    json!({
        "itemId": id(100 + n), "seq": n, "itemKind": "NOTICE", "address": agent, "runId": id(2),
        "noticeType": notice_type, "jobId": id(300 + n),
    })
}

pub(super) fn mail_view(n: u128, to: Vec<Uuid>, cc: Vec<Uuid>) -> Value {
    json!({ "success": true, "reason": null, "message": null, "mail": {
        "mailId": id(200 + n), "from": id(5), "author": "Ann", "sourceConversationId": id(6),
        "subject": "subject", "body": "body", "to": to, "cc": cc,
        "attachments": [{ "attachmentId": id(7), "name": "f.txt", "contentType": "text/plain", "size": 3 }],
    } })
}

pub(super) fn attachment_view(name: &str, content: &str) -> Value {
    json!({ "success": true, "reason": null, "message": null,
        "attachment": { "name": name, "contentType": "text/plain", "content": content } })
}

pub(super) fn count(jobs: &ScriptedJobs, root: &str) -> usize {
    jobs.calls()
        .iter()
        .filter(|call| matches!(call, Call::Graphql { document, .. } if document.contains(root)))
        .count()
}

pub(super) fn mark_reads(jobs: &ScriptedJobs) -> Vec<Vec<Uuid>> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::MarkRead { item_ids, .. } => Some(item_ids),
            _ => None,
        })
        .collect()
}

pub(super) async fn drained(queue: &DeliveryQueue) {
    tokio::time::timeout(Duration::from_secs(5), queue.drained())
        .await
        .expect("the queue drains");
}

pub(super) async fn yields() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}
