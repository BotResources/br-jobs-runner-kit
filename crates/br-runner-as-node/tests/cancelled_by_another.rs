mod support;

use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_runner_jobs::test_double::{
    Call, ChildCommand, ScriptedJobs, trigger_json, work_from_trigger,
};
use br_runner_jobs::{ArrivalSettings, IgnoreReports, QueueSettings, RunnerType};
use serde_json::{Value, json};
use support::*;
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn upload_accepted() -> Value {
    json!({ "jobsUploadPayload": { "success": true, "reason": null, "message": null } })
}

fn unread_cancel_notice(child: Uuid, message: &str) -> Value {
    json!({ "jobsUnread": { "success": true, "reason": null, "message": null, "items": [{
        "itemId": id(500), "seq": 1, "itemKind": "NOTICE", "address": null, "runId": id(2),
        "noticeType": "JOB_CANCELLED", "jobId": child, "message": message,
    }] } })
}

fn commands(jobs: &ScriptedJobs) -> Vec<ChildCommand> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Command(command) => Some(command),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn given_the_notice_of_a_child_job_cancelled_by_another_party_when_the_arrival_feed_reads_it_then_the_response_function_gets_cancelled_and_jobs_gets_no_finish_and_no_cancel()
 {
    let jobs = ScriptedJobs::new(
        RunnerType::new("runner").expect("a runner type"),
        "instance-a",
    );
    jobs.graphql_default(upload_accepted());
    let passport = Passport::human(
        id(5),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::new(),
    );
    let trigger = trigger_json(id(2), id(1), id(3), id(4), &passport).expect("a trigger");
    let work = work_from_trigger(&jobs, trigger).expect("a Work");
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
    let node = standard(source_of(&book.caller_handle()));
    let token = br_graph_contract::CancelToken::new();
    let mut run = start(&node, token.view()).await;
    run.give("count the tickets");

    let child = within(async {
        loop {
            if let Some(ChildCommand::Create(create)) = commands(&jobs).first().cloned() {
                return create.job_id;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    jobs.push_graphql(Ok(unread_cancel_notice(child, "Job was cancelled.")));
    let mut feed = work
        .arrival_feed(ArrivalSettings {
            ack_wait: Duration::from_secs(1),
            inactivity_limit: Duration::from_secs(60),
        })
        .await
        .expect("a feed");
    let pump = tokio::spawn(async move {
        while let Ok(arrival) = feed.next().await {
            book.arrival(&arrival);
        }
    });

    let update = update_of(run.event().await);
    queue.drained().await;
    pump.abort();

    assert_eq!(update.note.as_deref(), Some("cancelled:Job was cancelled."));
    let sent = commands(&jobs);
    assert_eq!(sent.len(), 1);
    assert!(matches!(&sent[0], ChildCommand::Create(create) if create.job_id == child));
}
