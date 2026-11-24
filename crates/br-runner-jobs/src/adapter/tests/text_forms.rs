use std::num::NonZeroUsize;

use br_agentic_contract::{IncomingPort, MailId};
use br_agentic_harness_contract::MailPort;

use super::support::id;
use super::{World, attachment_view, cc_item, mail_item, mail_view, notice_item, wire};
use crate::adapter::mark_gate_stored;
use br_runner_jobs_client::{Cause, Error, Failure};

const FORBIDDEN: [&str; 6] = [
    "passport",
    "token",
    "credential",
    "secret",
    "bearer",
    "authorization",
];

fn clean(text: &str) {
    let lower = text.to_lowercase();
    for word in FORBIDDEN {
        assert!(!lower.contains(word), "{word} in {text}");
    }
    assert!(
        !text.contains(&id(1).to_string()),
        "the Job id of the Work in {text}"
    );
}

#[tokio::test]
async fn given_the_adapters_the_entry_the_listed_items_and_the_errors_when_their_text_forms_are_read_then_none_shows_a_passport_a_token_a_credential_or_the_job_id_of_the_work()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let mail_world = World::mail().await;
    let mail_agent = mail_world.registered_agent().await;
    let mut items = vec![mail_item(agent.as_uuid(), 1), cc_item(agent.as_uuid(), 2)];
    items.push(notice_item(agent.as_uuid(), 3, "RUN_COMPLETED"));
    world.jobs.graphql_default(wire(
        items,
        mail_view(1, vec![agent.as_uuid()], Vec::new()),
        attachment_view("f.txt", "x"),
    ));
    let context = world.context(agent);
    let batch = world
        .incoming()
        .items(&context, None, NonZeroUsize::new(10).expect("limit"))
        .await
        .expect("a batch");
    clean(&format!("{batch:?}"));
    clean(&format!("{:?} {:?}", world.entry, mail_world.entry));
    clean(&format!("{:?}", world.entry.agent_port()));
    clean(&format!("{:?}", world.incoming()));
    clean(&format!("{:?}", mail_world.mail_port()));
    world.jobs.push_graphql(Err(Error::Lost(Failure::new(
        Cause::WaitFired,
        "no answer",
    ))));
    let error = world
        .incoming()
        .items(&context, None, NonZeroUsize::new(10).expect("limit"))
        .await
        .expect_err("an error");
    clean(&format!("{error:?} {error}"));
    let mail_context = mail_world.context(mail_agent);
    mail_world.jobs.push_graphql(Err(Error::Lost(Failure::new(
        Cause::WaitFired,
        "no answer",
    ))));
    let error = mail_world
        .mail_port()
        .documents(&mail_context, MailId::from_uuid(id(201)).expect("mail"))
        .await
        .expect_err("an error");
    clean(&format!("{error:?} {error}"));
    let stranger = mail_world.context(br_llm_messages::AgentId::from_uuid(id(77)).expect("agent"));
    let error = mail_world
        .mail_port()
        .items(&stranger, None, NonZeroUsize::new(1).expect("limit"))
        .await
        .expect_err("an error");
    clean(&format!("{error:?} {error}"));
}
