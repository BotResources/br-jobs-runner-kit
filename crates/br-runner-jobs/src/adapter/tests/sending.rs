use br_agentic_contract::{Attachment, DocumentRef, InboxPort, MailId, NewMail};
use br_llm_messages::{AgentId, Author, ConversationId};
use contract_jobs::graphql_roots::{SEND_MAIL_ROOT, UPLOAD_ATTACHMENT_ROOT};
use serde_json::{Value, json};

use super::{World, count, drained, refused, yields};
use crate::adapter::mark_gate_stored;
use crate::book::tests::{everything_succeeds, id, scripted};
use crate::error::Error;
use crate::queue::{QueueReport, Removal, WriteKind};
use crate::test_double::Call;

fn recipient(n: u128) -> AgentId {
    AgentId::from_uuid(id(n)).expect("agent")
}

fn mail(n: u128, to: Vec<AgentId>, cc: Vec<AgentId>) -> NewMail {
    NewMail {
        id: MailId::from_uuid(id(400 + n)).expect("mail id"),
        author: Author::new("Ann").expect("author"),
        source: ConversationId::from_uuid(id(6)).expect("conversation"),
        to,
        cc,
        subject: "subject".to_owned(),
        body: "body".to_owned(),
        attachments: vec![Attachment::new("a.txt", DocumentRef::new("doc://x"))],
    }
}

fn graphql_inputs(world: &World, root: &str) -> Vec<Value> {
    world
        .jobs
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Graphql {
                document,
                variables,
            } if document.contains(root) => Some(variables["input"].clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn given_a_mail_when_it_is_sent_then_the_call_returns_at_once_while_jobs_is_paused_and_the_queue_sends_the_mail_with_the_agent_as_sender_the_author_and_the_source_conversation()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.pause();
    let context = world.context(agent);
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        world
            .inbox()
            .send(&context, mail(1, vec![recipient(71)], vec![recipient(72)])),
    )
    .await
    .expect("the send returns at once")
    .expect("taken");
    assert_eq!(count(&world.jobs, SEND_MAIL_ROOT), 0);
    world.jobs.resume();
    drained(&world.queue).await;
    let sent = graphql_inputs(&world, SEND_MAIL_ROOT);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["mailId"], json!(id(401)));
    assert_eq!(sent[0]["fromAddress"], json!(agent.as_uuid()));
    assert_eq!(sent[0]["author"], json!("Ann"));
    assert_eq!(sent[0]["sourceConversationId"], json!(id(6)));
    assert_eq!(sent[0]["to"], json!([id(71)]));
    assert_eq!(sent[0]["cc"], json!([id(72)]));
}

#[tokio::test]
async fn given_an_attachment_when_the_mail_is_sent_then_the_document_reference_is_uploaded_as_text_plain_content_before_the_mail()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let context = world.context(agent);
    world
        .inbox()
        .send(&context, mail(1, vec![recipient(71)], Vec::new()))
        .await
        .expect("taken");
    drained(&world.queue).await;
    let uploads = graphql_inputs(&world, UPLOAD_ATTACHMENT_ROOT);
    assert_eq!(uploads.len(), 1);
    assert_eq!(uploads[0]["name"], json!("a.txt"));
    assert_eq!(uploads[0]["contentType"], json!("text/plain"));
    assert_eq!(uploads[0]["content"], json!("doc://x"));
    assert_eq!(uploads[0]["mailId"], json!(id(401)));
    let calls = world.jobs.calls();
    let position = |root: &str| {
        calls.iter().position(
            |call| matches!(call, Call::Graphql { document, .. } if document.contains(root)),
        )
    };
    assert!(position(UPLOAD_ATTACHMENT_ROOT) < position(SEND_MAIL_ROOT));
}

#[tokio::test]
async fn given_a_mail_id_the_book_took_when_the_mail_is_sent_again_then_the_call_succeeds_and_jobs_gets_one_mail()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let context = world.context(agent);
    for _ in 0..3 {
        world
            .inbox()
            .send(&context, mail(1, vec![recipient(71)], Vec::new()))
            .await
            .expect("taken");
    }
    drained(&world.queue).await;
    assert_eq!(count(&world.jobs, SEND_MAIL_ROOT), 1);
}

#[tokio::test]
async fn given_a_mail_without_a_recipient_in_to_or_with_a_recipient_twice_then_the_send_is_a_refused_mail_of_the_port_at_once_and_the_id_is_not_taken()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let context = world.context(agent);
    let none = world
        .inbox()
        .send(&context, mail(1, Vec::new(), vec![recipient(71)]))
        .await;
    let twice = world
        .inbox()
        .send(&context, mail(1, vec![recipient(71)], vec![recipient(71)]))
        .await;
    for error in [none, twice] {
        let error = error.expect_err("refused");
        assert!(error.is_refusal());
        assert!(!error.reason().expect("a reason").as_str().is_empty());
    }
    assert!(!world.book.mail_taken(id(401)));
    world
        .inbox()
        .send(&context, mail(1, vec![recipient(71)], Vec::new()))
        .await
        .expect("taken");
    drained(&world.queue).await;
    assert_eq!(count(&world.jobs, SEND_MAIL_ROOT), 1);
}

#[tokio::test]
async fn given_a_mail_whose_agent_registration_is_still_queued_then_the_send_returns_at_once_and_the_mail_waits_in_the_queue()
 {
    let jobs = scripted();
    jobs.answer_facts(|| Err(Error::lost("the started fact is not delivered")));
    let world = World::build(jobs, Vec::new(), mark_gate_stored()).await;
    let agent = world.agent().await;
    let context = world.context(agent);
    world
        .inbox()
        .send(&context, mail(1, vec![recipient(71)], Vec::new()))
        .await
        .expect("taken");
    yields().await;
    assert!(
        world
            .queue
            .state()
            .waiting
            .iter()
            .any(|name| name.kind == WriteKind::Mail)
    );
    assert_eq!(count(&world.jobs, SEND_MAIL_ROOT), 0);
}

#[tokio::test]
async fn given_a_mail_that_jobs_refuses_after_the_hand_over_then_the_send_was_a_success_and_the_refusal_reaches_the_observer()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let mut all = everything_succeeds();
    all[SEND_MAIL_ROOT] = refused("bad_addresses");
    world.jobs.graphql_default(all);
    let context = world.context(agent);
    world
        .inbox()
        .send(&context, mail(1, vec![recipient(71)], Vec::new()))
        .await
        .expect("taken");
    drained(&world.queue).await;
    let removed = world.reports.all().into_iter().any(|report| {
        matches!(&report, QueueReport::Removed { write, removal }
            if write.kind == WriteKind::Mail && matches!(removal, Removal::Refused(_)))
    });
    assert!(removed);
}

#[tokio::test]
async fn given_an_agent_that_holds_no_inbox_when_it_sends_then_the_call_is_an_error_of_the_port_and_the_id_is_not_taken()
 {
    let world = World::new(mark_gate_stored()).await;
    let unknown = world.context(recipient(777));
    let error = world
        .inbox()
        .send(&unknown, mail(1, vec![recipient(71)], Vec::new()))
        .await
        .expect_err("an error");
    assert!(!error.is_refusal());
    assert!(!world.book.mail_taken(id(401)));
}
