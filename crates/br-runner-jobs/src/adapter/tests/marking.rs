use std::num::NonZeroUsize;

use br_agentic_contract::InboxPort;
use br_llm_messages::{AgentId, EntryId};

use super::{
    ManualGate, World, cc_item, drained, mark_reads, notice_item, only_unread, thread, yields,
};
use crate::adapter::{AgentPortAdapter, mark_gate_stored};
use crate::book::tests::id;
use crate::queue::Gate;
use crate::test_double::{Call, ChildCommand};

fn entry(n: u128) -> EntryId {
    EntryId::from_uuid(id(n)).expect("entry")
}

async fn list_all(world: &World, agent: AgentId) -> usize {
    let context = world.context(agent);
    world
        .inbox()
        .items(&context, None, NonZeroUsize::new(20).expect("limit"))
        .await
        .expect("a batch")
        .items
        .len()
}

fn finish_count(world: &World) -> usize {
    world
        .jobs
        .calls()
        .iter()
        .filter(|call| matches!(call, Call::Command(ChildCommand::Finish { .. })))
        .count()
}

#[tokio::test]
async fn given_a_listed_mail_when_it_is_marked_then_the_call_returns_at_once_while_jobs_is_paused_and_the_item_leaves_the_listing_at_once()
 {
    let gate = ManualGate::new();
    let world = World::new(gate.gate()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(vec![
        cc_item(agent.as_uuid(), 1),
        cc_item(agent.as_uuid(), 2),
    ]));
    assert_eq!(list_all(&world, agent).await, 2);
    world.jobs.pause();
    let context = world.context(agent);
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        world.inbox().mark_read(&context, entry(201)),
    )
    .await
    .expect("the mark returns at once")
    .expect("marked");
    world.jobs.resume();
    assert_eq!(list_all(&world, agent).await, 1);
    assert!(mark_reads(&world.jobs).is_empty());
}

#[tokio::test]
async fn given_a_stored_gate_when_a_mail_is_marked_then_the_marker_of_the_item_of_jobs_not_of_the_mail_is_sent_and_the_gate_gets_the_thread_and_the_entry_id()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    list_all(&world, agent).await;
    let context = world.context(agent);
    world
        .inbox()
        .mark_read(&context, entry(201))
        .await
        .expect("marked");
    drained(&world.queue).await;
    assert_eq!(mark_reads(&world.jobs), vec![vec![id(101)]]);
    assert_eq!(finish_count(&world), 0);
}

#[tokio::test]
async fn given_a_gate_when_an_item_is_marked_then_it_is_asked_once_with_the_thread_of_the_context_and_the_entry_id()
 {
    let gate = ManualGate::new();
    let world = World::new(gate.gate()).await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    list_all(&world, agent).await;
    let context = world.context(agent);
    world
        .inbox()
        .mark_read(&context, entry(201))
        .await
        .expect("marked");
    world
        .inbox()
        .mark_read(&context, entry(201))
        .await
        .expect("marked again");
    yields().await;
    assert_eq!(gate.asked(), vec![(thread(), entry(201))]);
    gate.end(Gate::Stored);
    drained(&world.queue).await;
    assert_eq!(mark_reads(&world.jobs).len(), 1);
}

#[tokio::test]
async fn given_notices_that_are_not_run_completed_when_they_are_marked_then_no_finish_is_handed_over()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    let kinds = ["ACCEPTED", "QUEUE_POSITION", "STARTED", "JOB_FAILED"];
    let items = kinds
        .iter()
        .enumerate()
        .map(|(n, kind)| notice_item(agent.as_uuid(), n as u128 + 1, kind))
        .collect();
    world.jobs.graphql_default(only_unread(items));
    list_all(&world, agent).await;
    let context = world.context(agent);
    for n in 1..=4 {
        world
            .inbox()
            .mark_read(&context, entry(100 + n))
            .await
            .expect("marked");
    }
    drained(&world.queue).await;
    assert_eq!(mark_reads(&world.jobs).len(), 4);
    assert_eq!(finish_count(&world), 0);
}

#[tokio::test]
async fn given_a_pair_the_book_does_not_hold_when_it_is_marked_then_nothing_happens_and_there_is_no_error()
 {
    let gate = ManualGate::new();
    let world = World::new(gate.gate()).await;
    let agent = world.registered_agent().await;
    let context = world.context(agent);
    world
        .inbox()
        .mark_read(&context, entry(999))
        .await
        .expect("no error");
    yields().await;
    assert!(gate.asked().is_empty());
    assert!(world.queue.state().waiting.is_empty());
}

#[tokio::test]
async fn given_an_agent_that_holds_no_inbox_of_the_node_entry_when_it_marks_then_the_call_is_an_error_of_the_port()
 {
    let world = World::new(mark_gate_stored()).await;
    let helper = br_agentic_contract::AgentLabel::new("helper").expect("label");
    let other = AgentPortAdapter::clone(&world.entry.agent_port());
    let agent =
        br_agentic_contract::AgentPort::agent(&other, &super::Snap, super::instance(), &helper)
            .await
            .expect("agent");
    let context = world.context(agent);
    let error = world
        .inbox()
        .mark_read(&context, entry(201))
        .await
        .expect_err("an error");
    assert!(!error.is_refusal());
    let unknown = world.context(AgentId::from_uuid(id(777)).expect("agent"));
    assert!(world.inbox().mark_read(&unknown, entry(201)).await.is_err());
}

#[tokio::test]
async fn given_a_mail_with_two_recipient_agents_of_the_run_then_each_marks_its_own_item_under_the_one_mail_id()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    list_all(&world, agent).await;
    world.book.record_listed(
        id(72),
        id(201),
        crate::book::MailEntry {
            item_id: id(190),
            child_job_id: None,
            kind: crate::book::EntryKind::CcNotice,
        },
    );
    let context = world.context(agent);
    world
        .inbox()
        .mark_read(&context, entry(201))
        .await
        .expect("marked");
    drained(&world.queue).await;
    assert_eq!(mark_reads(&world.jobs), vec![vec![id(101)]]);
    assert!(!world.book.is_marked(id(72), id(190)));
}
