use br_agentic_contract::{Attachment, DocumentRef, MailId, NewMail};
use br_agentic_harness_contract::MailPort;
use br_llm_messages::{AgentId, Author, ConversationId};
use contract_jobs::graphql_roots::{SEND_MAIL_ROOT, UPLOAD_ATTACHMENT_ROOT};
use serde_json::Value;

use super::support::id;
use super::{World, count, drained};
use br_runner_jobs_client::test_double::Call;

pub(super) fn new_mail(n: u128) -> NewMail {
    NewMail {
        id: MailId::from_uuid(id(500 + n)).expect("mail id"),
        author: Author::new("Ann").expect("author"),
        source: ConversationId::from_uuid(id(6)).expect("conversation"),
        to: vec![AgentId::from_uuid(id(71)).expect("agent")],
        cc: vec![AgentId::from_uuid(id(72)).expect("agent")],
        subject: "subject".to_owned(),
        body: "body".to_owned(),
        attachments: Vec::new(),
    }
}

fn sends(world: &World) -> Vec<Value> {
    world
        .jobs
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Graphql {
                document,
                variables,
            } if document.contains(SEND_MAIL_ROOT) => Some(variables["input"].clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn given_a_mail_when_the_mail_port_sends_it_while_jobs_is_paused_then_the_call_returns_at_once_and_jobs_gets_the_mail_from_the_agent_of_the_context_later()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    world.jobs.pause();
    let context = world.context(agent);
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        world.mail_port().send(&context, new_mail(1)),
    )
    .await
    .expect("the send returns at once")
    .expect("accepted");
    assert!(sends(&world).is_empty());
    world.jobs.resume();
    drained(&world.queue).await;
    let sent = sends(&world);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["fromAddress"], serde_json::json!(agent.as_uuid()));
    assert_eq!(sent[0]["mailId"], serde_json::json!(id(501)));
    assert_eq!(sent[0]["author"], "Ann");
    assert_eq!(sent[0]["sourceConversationId"], serde_json::json!(id(6)));
    assert_eq!(sent[0]["to"], serde_json::json!([id(71)]));
    assert_eq!(sent[0]["cc"], serde_json::json!([id(72)]));
    assert_eq!(
        (sent[0]["subject"].as_str(), sent[0]["body"].as_str()),
        (Some("subject"), Some("body"))
    );
}

#[tokio::test]
async fn given_a_mail_with_attachments_when_it_is_sent_then_each_document_is_uploaded_under_its_name_with_the_reference_as_content_before_the_mail()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    let mut mail = new_mail(2);
    mail.attachments = vec![
        Attachment::new("a.txt", DocumentRef::new("first")),
        Attachment::new("b.txt", DocumentRef::new("second")),
    ];
    let context = world.context(agent);
    world
        .mail_port()
        .send(&context, mail)
        .await
        .expect("accepted");
    drained(&world.queue).await;
    let uploads: Vec<Value> = world
        .jobs
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Graphql {
                document,
                variables,
            } if document.contains(UPLOAD_ATTACHMENT_ROOT) => Some(variables["input"].clone()),
            _ => None,
        })
        .collect();
    let named: Vec<_> = uploads
        .iter()
        .map(|u| {
            (
                u["name"].as_str().unwrap_or(""),
                u["content"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(named, vec![("a.txt", "first"), ("b.txt", "second")]);
    assert_eq!(
        sends(&world)[0]["attachments"].as_array().map(Vec::len),
        Some(2)
    );
    let roots: Vec<&str> = world
        .jobs
        .calls()
        .iter()
        .filter_map(|call| match call {
            Call::Graphql { document, .. } if document.contains(UPLOAD_ATTACHMENT_ROOT) => {
                Some("upload")
            }
            Call::Graphql { document, .. } if document.contains(SEND_MAIL_ROOT) => Some("send"),
            _ => None,
        })
        .collect();
    assert_eq!(roots, vec!["upload", "upload", "send"]);
}

#[tokio::test]
async fn given_the_same_mail_id_when_it_is_sent_twice_then_it_is_handed_over_again_each_time_and_both_sends_reach_jobs()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    let context = world.context(agent);
    world
        .mail_port()
        .send(&context, new_mail(3))
        .await
        .expect("first");
    world
        .mail_port()
        .send(&context, new_mail(3))
        .await
        .expect("second");
    drained(&world.queue).await;
    let sent = sends(&world);
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0], sent[1]);
}

#[tokio::test]
async fn given_a_mail_with_no_recipient_in_to_or_a_recipient_listed_twice_when_it_is_sent_then_it_is_refused_with_a_reason_and_nothing_is_handed_over()
 {
    let world = World::mail().await;
    let agent = world.registered_agent().await;
    let context = world.context(agent);
    let mut none = new_mail(5);
    none.to.clear();
    let mut twice = new_mail(6);
    twice.cc = twice.to.clone();
    let mut repeated = new_mail(7);
    repeated.to.push(repeated.to[0]);
    for mail in [none, twice, repeated] {
        let error = world
            .mail_port()
            .send(&context, mail)
            .await
            .expect_err("a refusal");
        assert!(error.is_refusal());
        assert!(error.reason().is_some());
    }
    drained(&world.queue).await;
    assert_eq!(count(&world.jobs, SEND_MAIL_ROOT), 0);
}
