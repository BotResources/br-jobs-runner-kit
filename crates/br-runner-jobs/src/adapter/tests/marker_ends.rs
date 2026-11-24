use std::num::NonZeroUsize;

use br_agentic_contract::IncomingPort;
use br_llm_messages::{AgentId, EntryId};

use super::support::{id, scripted};
use super::world::restored_writer;
use super::{ManualGate, World, cc_item, drained, mark_reads, only_unread};
use crate::adapter::mark_gate_stored;
use br_runner_jobs_client::Gate;

fn entry(n: u128) -> EntryId {
    EntryId::from_uuid(id(n)).expect("entry")
}

async fn listed(world: &World, agent: AgentId) -> usize {
    let context = world.context(agent);
    world
        .incoming()
        .items(&context, None, NonZeroUsize::new(20).expect("limit"))
        .await
        .expect("a batch")
        .items
        .len()
}

async fn mark(world: &World, agent: AgentId, n: u128) {
    let context = world.context(agent);
    world
        .incoming()
        .mark_read(&context, entry(n))
        .await
        .expect("marked");
}

#[tokio::test]
async fn given_a_marker_held_by_a_gate_that_has_not_ended_when_the_agent_is_listed_then_the_item_is_left_out_and_nothing_reached_jobs()
 {
    let gate = ManualGate::new();
    let world = World::new(gate.gate()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(vec![
        cc_item(agent.as_uuid(), 1),
        cc_item(agent.as_uuid(), 2),
    ]));
    assert_eq!(listed(&world, agent).await, 2);
    mark(&world, agent, 201).await;
    assert_eq!(listed(&world, agent).await, 1);
    assert!(mark_reads(&world.jobs).is_empty());
}

#[tokio::test]
async fn given_a_marker_delivered_to_jobs_when_the_agent_is_listed_again_then_the_item_is_left_out_though_jobs_still_gives_it()
 {
    let world = World::new(mark_gate_stored()).await;
    let agent = world.registered_agent().await;
    world.jobs.graphql_default(only_unread(vec![
        cc_item(agent.as_uuid(), 1),
        cc_item(agent.as_uuid(), 2),
    ]));
    assert_eq!(listed(&world, agent).await, 2);
    mark(&world, agent, 201).await;
    drained(&world.queue).await;
    assert_eq!(mark_reads(&world.jobs), vec![vec![id(101)]]);
    assert_eq!(listed(&world, agent).await, 1);
}

#[tokio::test]
async fn given_a_marker_removed_after_a_dropped_gate_when_a_later_run_makes_a_new_book_then_the_item_is_listed_again()
 {
    let jobs = scripted();
    let gate = ManualGate::new();
    let first = World::build(jobs.clone(), vec![restored_writer()], gate.gate()).await;
    jobs.graphql_default(only_unread(vec![cc_item(id(70), 1)]));
    assert_eq!(
        listed(&first, AgentId::from_uuid(id(70)).expect("agent")).await,
        1
    );
    mark(&first, AgentId::from_uuid(id(70)).expect("agent"), 201).await;
    gate.end(Gate::Dropped);
    drained(&first.queue).await;
    assert_eq!(
        listed(&first, AgentId::from_uuid(id(70)).expect("agent")).await,
        0
    );
    assert!(mark_reads(&jobs).is_empty());
    let second = World::build(jobs.clone(), vec![restored_writer()], mark_gate_stored()).await;
    assert_eq!(
        listed(&second, AgentId::from_uuid(id(70)).expect("agent")).await,
        1
    );
}
