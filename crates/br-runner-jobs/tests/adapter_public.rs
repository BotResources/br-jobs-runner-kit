use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::{
    AgentLabel, AgentPort, Fields, InboxContext, InboxItem, InboxPort, InstanceId, MailItem,
    PortError, Reason, ThreadRef,
};
use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_llm_messages::EntryId;
use br_runner_jobs::test_double::{Call, ScriptedJobs, trigger_json, work_from_trigger};
use br_runner_jobs::{
    AgentBook, BookLookup, Gate, IgnoreReports, MarkGate, NodeEntry, QueueSettings, book_lookup,
    mark_gate,
};
use contract_jobs::graphql_roots::UNREAD_ROOT;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Snapshot;

impl Fields for Snapshot {
    type Update = ();

    fn empty_update() -> Self::Update {}

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, _update: Self::Update) -> Result<(), Reason> {
        Ok(())
    }
}

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

struct Run {
    jobs: Arc<ScriptedJobs>,
    queue: br_runner_jobs::DeliveryQueue,
    book: Arc<AgentBook>,
}

async fn run() -> Run {
    let jobs = ScriptedJobs::new("runner", "instance-a");
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
        trigger_json(id(2), id(1), id(3), id(4), &passport).expect("trigger"),
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
    Run { jobs, queue, book }
}

fn lookup_of(book: &Arc<AgentBook>) -> Arc<dyn BookLookup> {
    let book = Arc::clone(book);
    book_lookup(move |_instance| {
        let book = Arc::clone(&book);
        async move { Ok(book) }
    })
}

fn instance() -> InstanceId {
    InstanceId::new(id(900)).expect("instance")
}

fn label() -> AgentLabel {
    AgentLabel::new("writer").expect("label")
}

#[tokio::test]
async fn given_a_node_entry_when_the_agent_port_and_the_entry_ask_for_one_key_then_the_agent_id_is_the_same_and_a_lookup_error_is_a_port_error()
 {
    let run = run().await;
    let entry = NodeEntry::new("node", lookup_of(&run.book));
    assert!(entry.inbox_port().is_none());
    let from_entry = entry.agent_id(&run.book, &label()).expect("an agent");
    let from_port = entry
        .agent_port()
        .agent(&Snapshot, instance(), &label())
        .await
        .expect("an agent");
    assert_eq!(from_entry, from_port);
    let failing = NodeEntry::new(
        "node",
        book_lookup(|_| async { Err(PortError::fault("no book for this instance")) }),
    );
    assert!(
        failing
            .agent_port()
            .agent(&Snapshot, instance(), &label())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn given_a_node_with_inbox_when_an_item_is_listed_and_marked_then_the_marker_reaches_jobs_after_the_gate_ends_stored()
 {
    let run = run().await;
    let gate: MarkGate = mark_gate(|_, _| async { Gate::Stored });
    let entry = NodeEntry::with_inbox("node", lookup_of(&run.book), label(), gate);
    let agent = entry
        .agent_port()
        .agent(&Snapshot, instance(), &label())
        .await
        .expect("an agent");
    run.queue.drained().await;
    run.jobs.graphql_default(json!({ UNREAD_ROOT: {
        "success": true, "reason": null, "message": null,
        "items": [{ "itemId": id(101), "seq": 1, "itemKind": "CC_NOTICE", "address": agent.as_uuid(),
            "mailId": id(201), "subject": "subject", "attachments": null, "noticeType": null }],
    } }));
    let context = InboxContext {
        instance: instance(),
        thread: ThreadRef::from_uuid(id(901)).expect("thread"),
        agent,
    };
    let inbox = entry.inbox_port().expect("an inbox");
    let batch = inbox
        .items(&context, None, NonZeroUsize::new(10).expect("limit"))
        .await
        .expect("a batch");
    let InboxItem::Mail(MailItem::Cc { subject, .. }) = &batch.items[0] else {
        panic!("{batch:?}");
    };
    assert_eq!(subject, "subject");
    inbox
        .mark_read(&context, EntryId::from_uuid(id(201)).expect("entry"))
        .await
        .expect("marked");
    run.queue.drained().await;
    assert!(
        run.jobs.calls().iter().any(
            |call| matches!(call, Call::MarkRead { item_ids, .. } if item_ids == &vec![id(101)])
        )
    );
}
