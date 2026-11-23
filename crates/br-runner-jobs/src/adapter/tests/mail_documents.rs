use br_agentic_contract::MailId;
use br_agentic_harness_contract::MailPort;
use contract_jobs::graphql_roots::{ATTACHMENT_ROOT, MAIL_ROOT};
use serde_json::json;

use super::support::id;
use super::{World, attachment_view, count, mail_view, refused, wire, yields};
use br_runner_jobs_client::{Cause, Error, Failure};

fn mail(n: u128) -> MailId {
    MailId::from_uuid(id(200 + n)).expect("mail id")
}

fn with_two_attachments(to: Vec<uuid::Uuid>, cc: Vec<uuid::Uuid>) -> serde_json::Value {
    let mut view = mail_view(1, to, cc);
    view["mail"]["attachments"] = json!([
        { "attachmentId": id(7), "name": "a.txt", "contentType": "text/plain", "size": 1 },
        { "attachmentId": id(8), "name": "b.txt", "contentType": "text/plain", "size": 1 },
    ]);
    wire(Vec::new(), view, attachment_view("a.txt", "first"))
}

#[tokio::test]
async fn given_a_mail_with_two_attachments_when_the_documents_are_asked_then_the_mail_is_read_once_and_each_attachment_is_fetched_once()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(with_two_attachments(vec![agent.as_uuid()], Vec::new()));
    let context = world.context(agent);
    let documents = world
        .mail_port()
        .documents(&context, mail(1))
        .await
        .expect("documents");
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].name, "a.txt");
    assert_eq!(documents[0].content, b"first".to_vec());
    assert_eq!(count(&world.jobs, MAIL_ROOT), 1);
    assert_eq!(count(&world.jobs, ATTACHMENT_ROOT), 2);
}

#[tokio::test]
async fn given_a_mail_without_attachment_when_the_documents_are_asked_then_none_is_given_and_nothing_is_fetched()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    let mut view = mail_view(1, vec![agent.as_uuid()], Vec::new());
    view["mail"]["attachments"] = json!([]);
    world
        .jobs
        .graphql_default(wire(Vec::new(), view, attachment_view("a.txt", "x")));
    let context = world.context(agent);
    let documents = world
        .mail_port()
        .documents(&context, mail(1))
        .await
        .expect("documents");
    assert!(documents.is_empty());
    assert_eq!(count(&world.jobs, ATTACHMENT_ROOT), 0);
}

#[tokio::test]
async fn given_a_mail_where_the_agent_is_only_in_cc_when_the_documents_are_asked_then_the_call_is_refused_with_a_reason_and_nothing_is_fetched()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(with_two_attachments(vec![id(71)], vec![agent.as_uuid()]));
    let context = world.context(agent);
    let error = world
        .mail_port()
        .documents(&context, mail(1))
        .await
        .expect_err("a refusal");
    assert!(error.is_refusal() && error.reason().is_some());
    assert_eq!(count(&world.jobs, ATTACHMENT_ROOT), 0);
}

#[tokio::test]
async fn given_a_mail_that_jobs_says_is_not_the_agent_s_when_the_documents_are_asked_then_the_call_is_refused_with_the_text_of_jobs()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(wire(
        Vec::new(),
        refused("mail_not_yours"),
        refused("mail_not_yours"),
    ));
    let context = world.context(agent);
    let error = world
        .mail_port()
        .documents(&context, mail(1))
        .await
        .expect_err("a refusal");
    assert!(error.is_refusal());
}

#[tokio::test]
async fn given_a_read_of_the_mail_that_gets_no_answer_when_the_documents_are_asked_then_the_error_is_a_fault_of_the_port_and_no_attachment_is_fetched()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    world.jobs.push_graphql(Err(Error::Lost(Failure::new(
        Cause::WaitFired,
        "no answer",
    ))));
    let context = world.context(agent);
    let error = world
        .mail_port()
        .documents(&context, mail(1))
        .await
        .expect_err("an error");
    assert!(!error.is_refusal());
    yields().await;
    assert_eq!(count(&world.jobs, ATTACHMENT_ROOT), 0);
}

#[tokio::test]
async fn given_an_attachment_fetch_that_gets_no_answer_when_the_documents_are_asked_then_the_error_is_a_fault_of_the_port()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(with_two_attachments(vec![agent.as_uuid()], Vec::new()));
    world.jobs.push_graphql(Ok(wire(
        Vec::new(),
        mail_view(1, vec![agent.as_uuid()], Vec::new()),
        attachment_view("a.txt", "first"),
    )));
    world.jobs.push_graphql(Err(Error::Lost(Failure::new(
        Cause::WaitFired,
        "no answer",
    ))));
    let context = world.context(agent);
    let error = world
        .mail_port()
        .documents(&context, mail(1))
        .await
        .expect_err("an error");
    assert!(!error.is_refusal());
}
