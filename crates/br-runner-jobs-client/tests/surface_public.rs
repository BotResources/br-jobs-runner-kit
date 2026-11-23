use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::{InstanceId, PortError};
use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_runner_jobs_client::test_double::{ScriptedJobs, trigger_json, work_from_trigger};
use br_runner_jobs_client::{
    AgentBook, AgentKey, BookLookup, EntryKind, Gate, IgnoreReports, MailEntry, QueueSettings,
    QueueWriter, Recipients, RunnerType, Wake, Work, WorkCore, WorkIds, Write, WriteKind,
    book_lookup,
};
use uuid::Uuid;

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn known() -> InstanceId {
    InstanceId::new(id(60)).expect("an instance id")
}

fn runner_type() -> RunnerType {
    RunnerType::new("runner").expect("a runner type")
}

fn work(jobs: &Arc<ScriptedJobs>) -> Work {
    let passport = Passport::human(
        id(5),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::new(),
    );
    let trigger = trigger_json(id(2), id(1), id(3), id(4), &passport).expect("a trigger");
    work_from_trigger(jobs, trigger).expect("a Work")
}

async fn book_of(work: &Work) -> (Arc<AgentBook>, br_runner_jobs_client::DeliveryQueue) {
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
    let book = Arc::new(work.agent_book(&queue, Vec::new(), false).expect("a book"));
    (book, queue)
}

#[tokio::test]
async fn given_a_book_when_an_adapter_library_uses_its_public_methods_then_the_state_of_the_inbox_is_kept_for_the_life_of_the_book()
 {
    let jobs = ScriptedJobs::new(runner_type(), "instance-a");
    let work = work(&jobs);
    let (book, _queue) = book_of(&work).await;
    let key = AgentKey::new("node", "writer");
    let agent = book.agent_for(&key, true);
    let again = book.agent_for(&key, false);
    assert_eq!(agent.agent_id, again.agent_id);
    assert!(book.holds_inbox(agent.agent_id));
    assert_eq!(book.key_of(agent.agent_id), Some(key));
    assert!(book.registration_end(agent.agent_id).is_some());
    assert!(book.registration_queued(agent.agent_id));
    assert!(book.arrival_state().is_none());

    let entry = MailEntry {
        item_id: id(30),
        child_job_id: None,
        kind: EntryKind::DirectMail,
    };
    book.record_listed(agent.agent_id, id(31), entry);
    assert_eq!(book.listed(agent.agent_id, id(31)), Some(entry));
    assert!(book.take_mail(id(32)));
    assert!(!book.take_mail(id(32)));
    assert!(book.mark(agent.agent_id, id(30)));
    assert!(!book.mark(agent.agent_id, id(30)));
    assert!(book.marked_items(agent.agent_id).contains(&id(30)));
    book.keep_snapshot(agent.agent_id, Vec::new());
    assert_eq!(book.snapshot(agent.agent_id), Some(Vec::new()));
    book.drop_snapshot(agent.agent_id);
    assert_eq!(book.snapshot(agent.agent_id), None);
    let recipients = Recipients {
        to: vec![agent.agent_id],
        cc: Vec::new(),
    };
    book.keep_recipients(id(32), recipients.clone());
    assert_eq!(book.recipients(id(32)), Some(recipients));
    assert_eq!(book.core().run_id(), id(2));
}

#[tokio::test]
async fn given_a_watch_on_an_address_when_the_book_is_dropped_then_the_watch_gives_the_book_dropped_and_never_silence()
 {
    let jobs = ScriptedJobs::new(runner_type(), "instance-a");
    let work = work(&jobs);
    let (book, _queue) = book_of(&work).await;
    let mut watch = book.subscribe_items(id(40));
    drop(book);
    assert_eq!(watch.next().await, Wake::BookDropped);
}

#[tokio::test]
async fn given_a_lookup_made_from_an_async_function_when_it_is_awaited_then_it_gives_the_book_or_the_port_error()
 {
    let jobs = ScriptedJobs::new(runner_type(), "instance-a");
    let work = work(&jobs);
    let (book, _queue) = book_of(&work).await;
    let held = Arc::clone(&book);
    let lookup: Arc<dyn BookLookup> = book_lookup(move |instance: InstanceId| {
        let held = Arc::clone(&held);
        async move {
            if instance == known() {
                Ok(held)
            } else {
                Err(PortError::fault("no book"))
            }
        }
    });
    let found = lookup.book(known()).await.expect("a book");
    assert!(Arc::ptr_eq(&found, &book));
    assert!(
        lookup
            .book(InstanceId::new(id(61)).expect("an instance id"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn given_a_queue_writer_when_a_held_marker_is_stored_or_dropped_then_the_gate_releases_or_removes_it()
 {
    let jobs = ScriptedJobs::new(runner_type(), "instance-a");
    let work = work(&jobs);
    let (book, queue) = book_of(&work).await;
    let writer: QueueWriter = book.queue_writer();
    let agent = book
        .agent_for(&AgentKey::new("node", "writer"), true)
        .agent_id;
    let marker = |item: u128| Write::Marker {
        run_id: id(2),
        item_id: id(item),
        agent_id: agent,
        entry_id: id(item),
    };
    assert_eq!(marker(50).name().kind, WriteKind::ReadMarker);
    let stored = writer.hold(marker(50), None, Box::pin(async { Gate::Stored }));
    let dropped = writer.hold(marker(51), None, Box::pin(async { Gate::Dropped }));
    stored.await.expect("stored marker delivered");
    assert!(dropped.await.is_err());
    queue.drained().await;
}

#[tokio::test]
async fn given_a_work_when_its_core_is_read_then_the_ids_of_a_graphql_call_are_the_ones_of_the_work_and_the_backend_is_the_one_of_its_client()
 {
    let jobs = ScriptedJobs::new(runner_type(), "instance-a");
    let work = work(&jobs);
    let (book, _queue) = book_of(&work).await;
    let core: &Arc<WorkCore> = book.core();
    let ids: WorkIds = core.ids();
    assert_eq!(ids.job_id, id(1));
    assert_eq!(ids.run_id, id(2));
    assert_eq!(ids.instance_key, "instance-a");
    assert_eq!(core.run_id(), id(2));
}
