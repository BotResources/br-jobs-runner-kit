use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_runner_jobs_client::test_double::{ScriptedJobs, trigger_json, work_from_trigger};
use br_runner_jobs_client::{Error, IgnoreReports, MailHandle, QueueSettings, RunnerType, Work};
use contract_jobs::graphql_roots::UNREAD_ROOT;
use serde_json::{Value, json};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn work(jobs: &Arc<ScriptedJobs>) -> Work {
    let passport = Passport::human(
        id(5),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::new(),
    );
    let trigger = trigger_json(id(2), id(1), id(3), id(4), &passport).expect("a trigger");
    work_from_trigger(jobs, trigger).expect("a Work")
}

async fn handle(jobs: &Arc<ScriptedJobs>) -> MailHandle {
    let work = work(jobs);
    let (queue, started) = work
        .delivery_queue(QueueSettings {
            start_id: id(9),
            outcome_wait: Duration::from_secs(5),
            retry_wait: Duration::from_millis(10),
            observer: Arc::new(IgnoreReports),
        })
        .await
        .expect("a queue");
    started.await.expect("started");
    let book = work.agent_book(&queue, Vec::new(), false).expect("a book");
    book.core().mail_handle()
}

fn jobs_with(items: Vec<Value>) -> Arc<ScriptedJobs> {
    let jobs = ScriptedJobs::new(
        RunnerType::new("runner").expect("a runner type"),
        "instance-a",
    );
    jobs.push_graphql(Ok(json!({ UNREAD_ROOT: {
        "success": true, "reason": null, "message": null, "items": items,
    } })));
    jobs
}

fn item(n: u128, seq: Value, kind: &str) -> Value {
    json!({
        "itemId": id(100 + n), "seq": seq, "itemKind": kind, "address": id(70), "runId": id(2),
        "mailId": id(200 + n), "subject": "s", "from": id(5), "author": "Ann", "body": "b",
        "sourceConversationId": null, "attachments": [], "message": "m",
        "noticeType": "STARTED", "jobId": id(300 + n),
    })
}

#[tokio::test]
async fn given_unread_items_of_every_kind_when_listed_then_each_gives_the_seq_of_the_wire() {
    let kinds = ["MAIL", "CC_NOTICE", "NOTICE", "MAIL_NOTICE", "OTHER"];
    let wire = kinds
        .iter()
        .enumerate()
        .map(|(n, kind)| item(n as u128, json!(1_000_000_000_000_i64 + n as i64), kind))
        .collect();
    let jobs = jobs_with(wire);
    let items = handle(&jobs)
        .await
        .unread_sequenced(id(70))
        .await
        .expect("items");
    let seqs: Vec<i64> = items.iter().map(|found| found.seq).collect();
    assert_eq!(
        seqs,
        (0..5)
            .map(|n| 1_000_000_000_000_i64 + n)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn given_an_item_with_a_seq_that_is_missing_null_or_not_an_integer_when_listed_then_the_read_is_unreadable()
 {
    let mut missing = item(1, json!(1), "MAIL");
    missing.as_object_mut().expect("object").remove("seq");
    for bad in [
        missing,
        item(1, Value::Null, "MAIL"),
        item(1, json!("1"), "MAIL"),
        item(1, json!(1.5), "NOTICE"),
    ] {
        let jobs = jobs_with(vec![bad]);
        let error = handle(&jobs)
            .await
            .unread_sequenced(id(70))
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Unreadable(_)), "{error:?}");
    }
}

#[tokio::test]
async fn given_the_same_items_when_listed_with_and_without_the_seq_then_the_items_are_the_same() {
    let wire = || vec![item(1, json!(4), "MAIL"), item(2, json!(5), "NOTICE")];
    let plain = handle(&jobs_with(wire()))
        .await
        .unread(id(70))
        .await
        .expect("items");
    let sequenced = handle(&jobs_with(wire()))
        .await
        .unread_sequenced(id(70))
        .await
        .expect("items");
    let items: Vec<_> = sequenced.iter().map(|found| found.item.clone()).collect();
    assert_eq!(items, plain);
}
