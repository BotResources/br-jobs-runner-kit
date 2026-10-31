mod support;

use br_agentic_contract::{AiTurnRef, ThreadRef};
use br_llm_messages::{AgentId, DeltaOp, EntryId, ResponseDelta, ResponseMetadata};
use support::queue_kit::*;
use support::*;

fn started(turn: &AiTurnRef, entry: EntryId) -> ResponseDelta {
    ResponseDelta::new(
        turn.id,
        entry,
        DeltaOp::response_started(ResponseMetadata::new()),
    )
}

async fn open_with_one_delta(
    world: &World,
    thread: &ThreadRef,
    agent: AgentId,
    entry: EntryId,
) -> AiTurnRef {
    let turn = turn_ref(agent);
    let mut sink = world
        .access()
        .open_response(thread, turn.clone(), entry)
        .await
        .expect("accepted");
    sink.send(started(&turn, entry)).await;
    eventually("the delta was taken", || world.threads.chunks().len() == 1).await;
    turn
}

#[tokio::test]
async fn discarding_an_open_response_queues_the_delta_that_ends_it() {
    let world = world().await;
    let thread = thread_ref();
    let entry = entry_id();
    open_with_one_delta(&world, &thread, world.agent, entry).await;

    world
        .access()
        .discard_response(&thread, entry)
        .await
        .expect("accepted");

    eventually("the ending delta was taken", || {
        world.threads.chunks().len() == 2
    })
    .await;
}

#[tokio::test]
async fn after_the_end_of_an_instance_its_open_response_is_gone_so_a_discard_queues_nothing() {
    let world = world().await;
    let thread = thread_ref();
    let entry = entry_id();
    open_with_one_delta(&world, &thread, world.agent, entry).await;

    within("instance_ended", world.adapter.instance_ended(world.agent)).await;
    world
        .access()
        .discard_response(&thread, entry)
        .await
        .expect("accepted");
    quiet().await;

    assert_eq!(world.threads.chunks().len(), 1);
    assert_eq!(world.threads.commands().len(), 1);
}

#[tokio::test]
async fn the_end_of_an_instance_leaves_the_open_responses_of_other_agents() {
    let world = world().await;
    let thread = thread_ref();
    let (own, other) = (entry_id(), entry_id());
    let other_agent = agent_id();
    open_with_one_delta(&world, &thread, world.agent, own).await;
    let other_turn = turn_ref(other_agent);
    let mut sink = world
        .access()
        .open_response(&thread, other_turn.clone(), other)
        .await
        .expect("accepted");
    sink.send(started(&other_turn, other)).await;
    eventually("both deltas were taken", || {
        world.threads.chunks().len() == 2
    })
    .await;

    within("instance_ended", world.adapter.instance_ended(world.agent)).await;
    world
        .access()
        .discard_response(&thread, other)
        .await
        .expect("accepted");

    eventually("the ending delta of the other agent was taken", || {
        world.threads.chunks().len() == 3
    })
    .await;
}
