mod bad_items;
mod cancel;
mod live;
mod settings;

use std::sync::Arc;
use std::time::Duration;

use contract_jobs::graphql_roots::UNREAD_ROOT;
use futures::poll;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{Arrival, ArrivalFeed};
use crate::backend::{BoxFuture, RawFeed};
use crate::book::tests::*;
use crate::error::Error;
use crate::model::{ItemArrived, ItemKind, ResultRef, SyncEnd};
use crate::test_double::{Call, ScriptedJobs};

pub(super) struct Channel(mpsc::UnboundedReceiver<Result<ItemArrived, Error>>);

impl RawFeed<ItemArrived> for Channel {
    fn next(&mut self) -> BoxFuture<'_, Result<ItemArrived, Error>> {
        Box::pin(async move {
            self.0
                .recv()
                .await
                .unwrap_or_else(|| Err(Error::lost("the feed was dropped")))
        })
    }
}

pub(super) type Pusher = mpsc::UnboundedSender<Result<ItemArrived, Error>>;

pub(super) fn unread(items: Vec<Value>) -> Value {
    json!({ UNREAD_ROOT: { "success": true, "reason": null, "message": null, "items": items } })
}

pub(super) fn mail(n: u128) -> Value {
    json!({
        "itemId": id(500 + n), "seq": n, "itemKind": "MAIL", "address": id(70), "runId": id(RUN),
        "mailId": id(600 + n), "subject": "s", "from": id(71), "author": "Ann",
        "sourceConversationId": null, "body": "b", "attachments": [],
    })
}

pub(super) fn notice(n: u128, kind: &str, extra: Value) -> Value {
    let mut item = json!({
        "itemId": id(500 + n), "seq": n, "itemKind": "NOTICE", "address": null, "runId": id(RUN),
        "noticeType": kind, "jobId": id(700 + n),
    });
    if let (Some(target), Some(more)) = (item.as_object_mut(), extra.as_object()) {
        target.extend(more.clone());
    }
    item
}

pub(super) fn arrived(address: Option<Uuid>, n: u128, kind: ItemKind) -> ItemArrived {
    ItemArrived {
        address,
        item_id: id(500 + n),
        kind,
    }
}

pub(super) async fn open(jobs: &Arc<ScriptedJobs>, replay: Vec<Value>) -> (ArrivalFeed, Pusher) {
    jobs.push_graphql(Ok(unread(replay)));
    let work = work(jobs);
    let (pusher, receiver) = mpsc::unbounded_channel();
    let feed = ArrivalFeed::from_raw(Arc::clone(work.core()), Box::new(Channel(receiver)))
        .await
        .expect("a feed");
    (feed, pusher)
}

pub(super) async fn next(feed: &mut ArrivalFeed) -> Result<Arrival, Error> {
    tokio::time::timeout(Duration::from_secs(5), feed.next())
        .await
        .expect("an arrival")
}

pub(super) fn reads(jobs: &ScriptedJobs) -> usize {
    jobs.calls()
        .iter()
        .filter(|call| matches!(call, Call::Graphql { .. }))
        .count()
}

#[tokio::test]
async fn given_unread_items_when_the_feed_is_opened_then_they_replay_in_the_order_of_jobs_before_any_live_event()
 {
    let jobs = scripted();
    let (mut feed, pusher) = open(&jobs, vec![notice(2, "STARTED", json!({})), mail(1)]).await;
    pusher
        .send(Ok(arrived(Some(id(70)), 3, ItemKind::DirectMail)))
        .expect("sent");
    let kinds: Vec<_> = {
        let mut seen = Vec::new();
        for _ in 0..3 {
            let Arrival::Item(item) = next(&mut feed).await.expect("an item") else {
                panic!("an item is expected");
            };
            seen.push((item.item_id, item.kind));
        }
        seen
    };
    assert_eq!(
        kinds,
        vec![
            (id(502), ItemKind::CallNotice),
            (id(501), ItemKind::DirectMail),
            (id(503), ItemKind::DirectMail),
        ]
    );
    assert_eq!(reads(&jobs), 1);
}

#[tokio::test]
async fn given_a_run_level_completion_in_the_replay_when_the_feed_is_opened_then_its_sync_end_follows_its_item()
 {
    let jobs = scripted();
    let (mut feed, _pusher) = open(
        &jobs,
        vec![notice(1, "RUN_COMPLETED", json!({ "resultId": id(800) }))],
    )
    .await;
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(_))));
    assert_eq!(
        next(&mut feed).await,
        Ok(Arrival::SyncEnd {
            child_job_id: id(701),
            end: SyncEnd::Completed {
                inline: None,
                result_ref: Some(ResultRef(id(800)))
            },
        })
    );
}

#[tokio::test]
async fn given_a_run_level_failure_in_the_replay_then_the_report_and_the_message_come_with_the_sync_end()
 {
    let jobs = scripted();
    let (mut feed, _pusher) = open(
        &jobs,
        vec![notice(
            1,
            "JOB_FAILED",
            json!({ "message": "It failed.", "failureReport": {
                "kind": "PERMANENT", "reason_code": "bad", "params": {}, "diagnostic": "d" } }),
        )],
    )
    .await;
    let _ = next(&mut feed).await;
    let Ok(Arrival::SyncEnd {
        child_job_id,
        end: SyncEnd::Failed { report, message },
    }) = next(&mut feed).await
    else {
        panic!("a failure is expected");
    };
    assert_eq!(child_job_id, id(701));
    assert_eq!(message, "It failed.");
    assert_eq!(report.expect("a report").reason_code, "bad");
}

#[tokio::test]
async fn given_a_notice_for_an_agent_address_in_the_replay_then_no_sync_end_is_made() {
    let jobs = scripted();
    let mut item = notice(1, "RUN_COMPLETED", json!({ "resultId": id(800) }));
    item["address"] = json!(id(70));
    let (mut feed, pusher) = open(&jobs, vec![item]).await;
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(_))));
    pusher
        .send(Ok(arrived(Some(id(70)), 9, ItemKind::DirectMail)))
        .expect("sent");
    assert!(matches!(next(&mut feed).await, Ok(Arrival::Item(item)) if item.item_id == id(509)));
}
