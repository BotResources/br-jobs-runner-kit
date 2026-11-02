#[path = "../examples/agent_and_inbox.rs"]
#[allow(dead_code)]
mod agent_and_inbox;

use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::{AgentLabel, InboxContext, InstanceId, ThreadRef};
use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_llm_messages::{AgentId, Author, ConversationId};
use br_runner_jobs::test_double::{Call, ScriptedJobs, trigger_json, work_from_trigger};
use br_runner_jobs::{AgentBook, DeliveryQueue, IgnoreReports, NodeEntry, QueueSettings};
use contract_jobs::graphql_roots::{SEND_MAIL_ROOT, UNREAD_ROOT, UPLOAD_ATTACHMENT_ROOT};
use serde_json::{Value, json};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn success() -> Value {
    json!({ "success": true, "reason": null, "message": null })
}

struct World {
    jobs: Arc<ScriptedJobs>,
    queue: DeliveryQueue,
    book: Arc<AgentBook>,
    entry: NodeEntry,
    label: AgentLabel,
}

async fn world() -> World {
    let jobs = ScriptedJobs::new(
        br_runner_jobs::RunnerType::new("analyst").expect("a runner type"),
        "pod-1",
    );
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
    let entry = agent_and_inbox::build_node_entry(Arc::clone(&book), label.clone());
    World {
        jobs,
        queue,
        book,
        entry,
        label,
    }
}

fn instance() -> InstanceId {
    InstanceId::new(id(900)).expect("an instance")
}

#[tokio::test]
async fn the_listed_mail_is_marked_and_its_marker_reaches_jobs() {
    let world = world().await;
    let agent = world
        .entry
        .agent_id(&world.book, &world.label)
        .expect("an agent");
    world.queue.drained().await;
    world.jobs.graphql_default(json!({ UNREAD_ROOT: {
        "success": true, "reason": null, "message": null,
        "items": [{ "itemId": id(101), "seq": 1, "itemKind": "CC_NOTICE", "address": agent.as_uuid(),
            "mailId": id(201), "subject": "subject", "attachments": null, "noticeType": null }],
    } }));

    let listed = agent_and_inbox::read_the_inbox(
        &world.entry,
        instance(),
        ThreadRef::from_uuid(id(901)).expect("a thread"),
        &world.label,
        &world.book,
    )
    .await
    .expect("a listing");

    world.queue.drained().await;
    assert_eq!(listed, 1);
    assert!(
        world.jobs.calls().iter().any(
            |call| matches!(call, Call::MarkRead { item_ids, .. } if item_ids == &vec![id(101)])
        )
    );
}

#[tokio::test]
async fn the_mail_of_the_example_reaches_jobs_from_the_agent() {
    let world = world().await;
    world.jobs.graphql_default(json!({
        SEND_MAIL_ROOT: success(),
        UPLOAD_ATTACHMENT_ROOT: success(),
    }));
    let agent = world
        .entry
        .agent_id(&world.book, &world.label)
        .expect("an agent");
    let context = InboxContext {
        instance: instance(),
        thread: ThreadRef::from_uuid(id(901)).expect("a thread"),
        agent,
    };

    agent_and_inbox::send_a_mail(
        &world.entry,
        &context,
        ConversationId::from_uuid(id(6)).expect("a conversation"),
        vec![AgentId::from_uuid(id(77)).expect("a recipient")],
        Author::new("Ann").expect("an author"),
    )
    .await
    .expect("a mail");
    world.queue.drained().await;

    let sent = world
        .jobs
        .calls()
        .into_iter()
        .filter(|call| matches!(call, Call::Graphql { document, .. } if document.contains(SEND_MAIL_ROOT)))
        .count();
    assert_eq!(sent, 1);
}
