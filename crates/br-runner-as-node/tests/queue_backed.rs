mod support;

use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_graph_contract::Event;
use br_runner_jobs::test_double::{
    Call, ChildCommand, ScriptedJobs, trigger_json, work_from_trigger,
};
use br_runner_jobs::{Arrival, Error as JobsError, IgnoreReports, QueueSettings, SyncEnd};
use serde_json::{Value, json};
use support::*;
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn succeeds(root: &str) -> Value {
    json!({ root: { "success": true, "reason": null, "message": null } })
}

fn fails(root: &str, reason: &str) -> Value {
    json!({ root: { "success": false, "reason": reason, "message": "no" } })
}

struct Stack {
    jobs: Arc<ScriptedJobs>,
    book: br_runner_jobs::AgentBook,
    queue: br_runner_jobs::DeliveryQueue,
}

async fn stack(upload: Value) -> Stack {
    let jobs = ScriptedJobs::new("runner", "instance-a");
    jobs.graphql_default(upload);
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
    let settings = QueueSettings {
        start_id: id(9),
        outcome_wait: Duration::from_secs(5),
        retry_wait: Duration::from_millis(10),
        observer: Arc::new(IgnoreReports),
    };
    let (queue, started) = work.delivery_queue(settings).await.expect("a queue");
    started.await.expect("started");
    let book = work.agent_book(&queue, Vec::new(), false).expect("a book");
    Stack { jobs, book, queue }
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

async fn refused_claim(reason: &str) -> JobsError {
    let jobs = ScriptedJobs::new("analyst", "pod-1");
    let body = format!(r#"{{"version":2,"reason":"{reason}","retry_after_ms":250}}"#);
    jobs.push_claim_reply(Ok(body.into_bytes()));
    jobs.client()
        .claim(Uuid::now_v7(), Duration::from_secs(1))
        .await
        .expect_err("a refusal")
}

#[tokio::test]
async fn given_the_real_delivery_queue_when_the_runner_completes_then_jobs_gets_the_upload_the_create_and_the_finish_in_order_and_never_a_cancel()
 {
    let stack = stack(succeeds("jobsUploadPayload")).await;
    let node = standard(source_of(&stack.book.caller_handle()));
    let token = br_graph_contract::CancelToken::new();
    let mut run = start(&node, token.view()).await;
    run.give("count the tickets");

    let child = within(async {
        loop {
            if let Some(ChildCommand::Create(create)) = commands(&stack.jobs).first().cloned() {
                return create.job_id;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    stack.book.arrival(&Arrival::SyncEnd {
        child_job_id: child,
        end: SyncEnd::Completed {
            inline: Some(content("42 tickets")),
            result_ref: None,
        },
    });
    let update = update_of(run.event().await);
    stack.queue.drained().await;

    assert_eq!(update.answer.as_deref(), Some("42 tickets"));
    let sent = commands(&stack.jobs);
    assert!(matches!(&sent[0], ChildCommand::Create(create) if create.job_id == child));
    assert!(matches!(&sent[1], ChildCommand::Finish { job_id, .. } if *job_id == child));
    assert_eq!(sent.len(), 2);
}

#[tokio::test]
async fn given_a_refused_upload_when_the_real_queue_drops_the_create_then_the_outcome_is_the_refusal_and_jobs_never_gets_the_create_or_a_cancel()
 {
    let stack = stack(fails("jobsUploadPayload", "payload_too_big")).await;
    let node = standard(source_of(&stack.book.caller_handle()));
    let token = br_graph_contract::CancelToken::new();
    let mut run = start(&node, token.view()).await;

    run.give("q");
    let update = update_of(run.event().await);
    stack.queue.drained().await;

    assert_eq!(update.note.as_deref(), Some("refused:payload_too_big"));
    assert!(commands(&stack.jobs).is_empty());
}

#[tokio::test]
async fn given_a_create_that_jobs_refuses_when_the_real_queue_sends_it_then_the_outcome_is_the_refusal_and_no_cancel_follows()
 {
    let stack = stack(succeeds("jobsUploadPayload")).await;
    let refusal = refused_claim("unknown_runner_type").await;
    stack.jobs.answer_commands(move || Err(refusal.clone()));
    let node = standard(source_of(&stack.book.caller_handle()));
    let token = br_graph_contract::CancelToken::new();
    let mut run = start(&node, token.view()).await;

    run.give("q");
    let update = update_of(run.event().await);
    stack.queue.drained().await;

    assert_eq!(update.note.as_deref(), Some("refused:unknown_runner_type"));
    assert!(
        commands(&stack.jobs)
            .iter()
            .all(|command| matches!(command, ChildCommand::Create(_)))
    );
}

#[tokio::test]
async fn given_the_cancel_view_when_the_real_queue_holds_a_delivered_create_then_jobs_gets_the_create_and_then_the_cancel_of_the_same_child()
 {
    let stack = stack(succeeds("jobsUploadPayload")).await;
    let node = standard(source_of(&stack.book.caller_handle()));
    let token = br_graph_contract::CancelToken::new();
    let mut run = start(&node, token.view()).await;
    run.give("q");
    within(async {
        while commands(&stack.jobs).is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;

    token.handle().trigger();
    assert!(matches!(run.event().await, Event::Cancelled));
    stack.queue.drained().await;

    let sent = commands(&stack.jobs);
    assert_eq!(sent.len(), 2);
    let ChildCommand::Create(create) = &sent[0] else {
        panic!("the create comes first");
    };
    assert!(matches!(&sent[1], ChildCommand::Cancel { job_id, .. } if *job_id == create.job_id));
}
