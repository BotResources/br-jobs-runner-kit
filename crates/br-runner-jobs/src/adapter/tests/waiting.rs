use std::time::Duration;

use br_agentic_contract::InboxPort;
use br_llm_messages::AgentId;
use contract_jobs::graphql_roots::UNREAD_ROOT;
use futures::FutureExt;

use super::{World, cc_item, count, drained, only_unread, yields};
use crate::adapter::mark_gate_stored;
use crate::arrival::Arrival;
use crate::book::tests::{id, scripted};
use crate::error::Error;
use crate::model::{ItemArrived, ItemKind};

fn arrived(agent: AgentId, n: u128) -> Arrival {
    Arrival::Item(ItemArrived {
        address: Some(agent.as_uuid()),
        item_id: id(100 + n),
        kind: ItemKind::CcNotice,
    })
}

async fn finished<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("the wait ends")
}

#[tokio::test]
async fn given_an_unread_item_when_a_wait_is_opened_then_it_returns_after_one_read() {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    let context = world.context(agent);
    finished(world.inbox().wait(&context)).await.expect("woken");
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
}

#[tokio::test]
async fn given_an_empty_inbox_when_an_item_arrives_then_the_wait_reads_again_and_returns_and_without_an_event_nothing_polls()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(Vec::new()));
    let context = world.context(agent);
    let inbox = world.inbox();
    let mut wait = Box::pin(inbox.wait(&context));
    assert!(wait.as_mut().now_or_never().is_none());
    yields().await;
    assert!(wait.as_mut().now_or_never().is_none());
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    world.book.arrival(&arrived(agent, 1));
    finished(wait).await.expect("woken");
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 2);
}

#[tokio::test]
async fn given_an_event_for_another_address_or_for_an_item_the_agent_marked_then_the_wait_stays_open()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    let context = world.context(agent);
    world
        .inbox()
        .items(
            &context,
            None,
            std::num::NonZeroUsize::new(5).expect("limit"),
        )
        .await
        .expect("listed");
    world
        .inbox()
        .mark_read(
            &context,
            br_llm_messages::EntryId::from_uuid(id(201)).expect("entry"),
        )
        .await
        .expect("marked");
    let reads = count(&world.jobs, UNREAD_ROOT);
    let inbox = world.inbox();
    let mut wait = Box::pin(inbox.wait(&context));
    assert!(wait.as_mut().now_or_never().is_none());
    yields().await;
    world.book.arrival(&Arrival::Item(ItemArrived {
        address: Some(id(71)),
        item_id: id(190),
        kind: ItemKind::CcNotice,
    }));
    yields().await;
    assert!(wait.as_mut().now_or_never().is_none());
    assert_eq!(count(&world.jobs, UNREAD_ROOT), reads + 1);
    world.book.arrival(&arrived(agent, 1));
    yields().await;
    assert!(wait.as_mut().now_or_never().is_none());
    assert_eq!(count(&world.jobs, UNREAD_ROOT), reads + 2);
}

#[tokio::test]
async fn given_a_new_agent_whose_registration_is_queued_when_it_ends_then_the_wait_reads_without_an_event_and_returns()
 {
    let jobs = scripted();
    jobs.answer_facts(|| Err(Error::lost("the started fact is not delivered")));
    let world = World::build(jobs, Vec::new(), mark_gate_stored()).await;
    let agent = world.agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    let context = world.context(agent);
    let inbox = world.inbox();
    let mut wait = Box::pin(inbox.wait(&context));
    assert!(wait.as_mut().now_or_never().is_none());
    yields().await;
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 0);
    world.jobs.answer_facts(|| Ok(()));
    finished(wait)
        .await
        .expect("woken by the end of the registration");
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
    drained(&world.queue).await;
}

#[tokio::test]
async fn given_the_end_of_the_feed_with_an_error_when_a_wait_is_open_then_it_ends_with_an_error_of_the_port()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(Vec::new()));
    let context = world.context(agent);
    let inbox = world.inbox();
    let mut wait = Box::pin(inbox.wait(&context));
    assert!(wait.as_mut().now_or_never().is_none());
    world.book.arrival_ended(&Error::lost("the feed was cut"));
    let error = finished(wait).await.expect_err("an error");
    assert!(!error.is_refusal());
}

#[tokio::test]
async fn given_a_book_that_keeps_infra_missing_when_a_wait_opens_with_no_item_then_it_is_an_error_but_with_an_item_it_returns()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world
        .book
        .arrival_ended(&Error::InfraMissing("gone".to_owned()));
    world.jobs.graphql_default(only_unread(Vec::new()));
    let context = world.context(agent);
    assert!(finished(world.inbox().wait(&context)).await.is_err());
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    finished(world.inbox().wait(&context))
        .await
        .expect("an item is there");
}

#[tokio::test]
async fn given_a_feed_that_ended_with_lost_when_a_wait_opens_then_it_stays_open_until_the_feed_is_open_again_and_an_item_arrives()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(Vec::new()));
    world.book.arrival_ended(&Error::lost("the feed was cut"));
    let context = world.context(agent);
    let inbox = world.inbox();
    let mut wait = Box::pin(inbox.wait(&context));
    assert!(wait.as_mut().now_or_never().is_none());
    yields().await;
    assert!(wait.as_mut().now_or_never().is_none());
    world.book.arrival_open_again();
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    world.book.arrival(&arrived(agent, 1));
    finished(wait).await.expect("woken after the replay");
}

#[tokio::test]
async fn given_a_read_with_no_answer_when_a_wait_is_open_then_it_is_an_error_of_the_port_and_the_read_is_not_repeated()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.push_graphql(Err(Error::wait_fired("no answer")));
    let context = world.context(agent);
    let error = finished(world.inbox().wait(&context))
        .await
        .expect_err("an error");
    assert!(!error.is_refusal());
    yields().await;
    assert_eq!(count(&world.jobs, UNREAD_ROOT), 1);
}

#[tokio::test]
async fn given_an_abandoned_wait_when_an_item_arrives_then_the_next_wait_loses_nothing() {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(Vec::new()));
    let context = world.context(agent);
    {
        let inbox = world.inbox();
        let mut abandoned = Box::pin(inbox.wait(&context));
        assert!(abandoned.as_mut().now_or_never().is_none());
    }
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    world.book.arrival(&arrived(agent, 1));
    finished(world.inbox().wait(&context))
        .await
        .expect("the item is still there");
}

#[tokio::test]
async fn given_a_wait_dropped_while_jobs_is_slow_then_a_new_wait_reads_normally() {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world
        .jobs
        .graphql_default(only_unread(vec![cc_item(agent.as_uuid(), 1)]));
    world.jobs.pause();
    let context = world.context(agent);
    {
        let inbox = world.inbox();
        let mut slow = Box::pin(inbox.wait(&context));
        assert!(slow.as_mut().now_or_never().is_none());
        yields().await;
    }
    world.jobs.resume();
    finished(world.inbox().wait(&context)).await.expect("woken");
}
