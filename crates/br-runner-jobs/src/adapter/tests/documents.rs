use br_agentic_contract::{InboxPort, MailId};
use contract_jobs::graphql_roots::{ATTACHMENT_ROOT, MAIL_ROOT};

use super::{World, attachment_view, count, mail_view, only_unread, refused, wire};
use crate::adapter::mark_gate_stored;
use crate::book::tests::id;
use crate::error::Error;

fn mail() -> MailId {
    MailId::from_uuid(id(201)).expect("mail id")
}

#[tokio::test]
async fn given_a_mail_with_the_agent_in_to_when_the_documents_are_asked_then_each_attachment_is_fetched_and_its_content_comes_back_as_bytes()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(wire(
        Vec::new(),
        mail_view(1, vec![agent.as_uuid()], Vec::new()),
        attachment_view("f.txt", "hello"),
    ));
    let context = world.context(agent);
    let documents = world
        .inbox()
        .documents(&context, mail())
        .await
        .expect("documents");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].name, "f.txt");
    assert_eq!(documents[0].content, b"hello".to_vec());
    assert_eq!(count(&world.jobs, MAIL_ROOT), 1);
    assert_eq!(count(&world.jobs, ATTACHMENT_ROOT), 1);
}

#[tokio::test]
async fn given_a_mail_where_the_agent_is_only_in_cc_when_the_documents_are_asked_then_the_request_is_refused_and_no_attachment_is_fetched()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(wire(
        Vec::new(),
        mail_view(1, vec![id(71)], vec![agent.as_uuid()]),
        attachment_view("f.txt", "hello"),
    ));
    let context = world.context(agent);
    let error = world
        .inbox()
        .documents(&context, mail())
        .await
        .expect_err("refused");
    assert!(error.is_refusal());
    assert_eq!(count(&world.jobs, ATTACHMENT_ROOT), 0);
}

#[tokio::test]
async fn given_a_mail_that_jobs_does_not_hold_when_the_documents_are_asked_then_the_request_is_refused_with_the_text_of_jobs()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(Vec::new()));
    let context = world.context(agent);
    let error = world
        .inbox()
        .documents(&context, mail())
        .await
        .expect_err("refused");
    assert!(error.is_refusal());
    assert_eq!(
        error.reason().expect("a reason").as_str(),
        "The mail was not sent to this address."
    );
}

#[tokio::test]
async fn given_another_refusal_of_jobs_when_the_documents_are_asked_then_it_is_a_fault_and_not_a_refused_request()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(wire(
        Vec::new(),
        refused("address_not_yours"),
        attachment_view("f.txt", "hello"),
    ));
    let context = world.context(agent);
    let error = world
        .inbox()
        .documents(&context, mail())
        .await
        .expect_err("a fault");
    assert!(!error.is_refusal());
}

#[tokio::test]
async fn given_no_answer_from_jobs_when_the_documents_are_asked_then_it_is_a_fault_and_nothing_is_read_again()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.push_graphql(Err(Error::wait_fired("no answer")));
    let context = world.context(agent);
    let error = world
        .inbox()
        .documents(&context, mail())
        .await
        .expect_err("a fault");
    assert!(!error.is_refusal());
    assert_eq!(count(&world.jobs, MAIL_ROOT), 1);
    assert_eq!(count(&world.jobs, ATTACHMENT_ROOT), 0);
}

#[tokio::test]
async fn given_an_attachment_that_cannot_be_fetched_when_the_documents_are_asked_then_it_is_a_fault()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.push_graphql(Ok(wire(
        Vec::new(),
        mail_view(1, vec![agent.as_uuid()], Vec::new()),
        attachment_view("f.txt", "hello"),
    )));
    world.jobs.push_graphql(Err(Error::wait_fired("no answer")));
    let context = world.context(agent);
    let error = world
        .inbox()
        .documents(&context, mail())
        .await
        .expect_err("a fault");
    assert!(!error.is_refusal());
}
