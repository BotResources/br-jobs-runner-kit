#[path = "../examples/agent_and_mail.rs"]
mod agent_and_mail;

use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::{AgentLabel, InstanceId, MailId, NewMail, ThreadRef};
use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_llm_messages::{AgentId, Author, ConversationId};
use br_runner_jobs::NodeEntry;
use br_runner_jobs_client::test_double::{Call, ScriptedJobs, trigger_json, work_from_trigger};
use br_runner_jobs_client::{AgentBook, DeliveryQueue, IgnoreReports, QueueSettings, RunnerType};
use contract_jobs::graphql_roots::{SEND_MAIL_ROOT, UNREAD_ROOT};
use serde_json::json;
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

struct World {
    jobs: Arc<ScriptedJobs>,
    queue: DeliveryQueue,
    book: Arc<AgentBook>,
    entry: NodeEntry,
    label: AgentLabel,
}

async fn world() -> World {
    let jobs = ScriptedJobs::new(RunnerType::new("analyst").expect("a runner type"), "pod-1");
    let passport = Passport::human(
        id(5),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::new(),
    );
    let work = work_from_trigger(
        &jobs,
        trigger_json(id(2), id(1), id(3), id(4), &passport).expect("a trigger"),
    )
    .expect("a Work");
    let settings = QueueSettings {
        start_id: id(9),
        outcome_wait: Duration::from_secs(5),
        retry_wait: Duration::from_millis(10),
        observer: Arc::new(IgnoreReports),
    };
    let (queue, _) = work.delivery_queue(settings).await.expect("a queue");
    let book = Arc::new(work.agent_book(&queue, Vec::new(), false).expect("a book"));
    let label = AgentLabel::new("writer").expect("a label");
    let entry = agent_and_mail::build_node_entry(Arc::clone(&book), label.clone());
    World {
        jobs,
        queue,
        book,
        entry,
        label,
    }
}

fn reply() -> NewMail {
    NewMail {
        id: MailId::from_uuid(id(600)).expect("a mail id"),
        author: Author::new("Ann").expect("an author"),
        source: ConversationId::from_uuid(id(6)).expect("a conversation"),
        to: vec![AgentId::from_uuid(id(71)).expect("an agent")],
        cc: Vec::new(),
        subject: "re".to_owned(),
        body: "done".to_owned(),
        attachments: Vec::new(),
    }
}

#[test]
fn the_example_has_a_main() {
    agent_and_mail::main();
}

#[tokio::test]
async fn the_mail_is_listed_the_reply_is_sent_and_the_marker_reaches_jobs_with_no_gate() {
    let world = world().await;
    let agent = world
        .entry
        .agent_id(&world.book, &world.label)
        .expect("an agent");
    world.queue.drained().await;
    world.jobs.graphql_default(json!({
        UNREAD_ROOT: { "success": true, "reason": null, "message": null,
            "items": [{ "itemId": id(101), "seq": 1, "itemKind": "CC_NOTICE", "address": agent.as_uuid(),
                "mailId": id(201), "subject": "subject", "attachments": null, "noticeType": null }] },
        SEND_MAIL_ROOT: { "success": true, "reason": null, "message": null },
    }));

    let listed = agent_and_mail::read_send_and_mark(
        &world.entry,
        InstanceId::new(id(900)).expect("an instance"),
        ThreadRef::from_uuid(id(901)).expect("a thread"),
        &world.label,
        &world.book,
        reply(),
    )
    .await
    .expect("a listing");

    world.queue.drained().await;
    let calls = world.jobs.calls();
    assert_eq!(listed, 1);
    assert!(calls.iter().any(
        |call| matches!(call, Call::Graphql { document, .. } if document.contains(SEND_MAIL_ROOT))
    ));
    assert!(
        calls.iter().any(
            |call| matches!(call, Call::MarkRead { item_ids, .. } if item_ids == &vec![id(101)])
        )
    );
}
