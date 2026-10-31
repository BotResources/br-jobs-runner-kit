mod support;

#[path = "../examples/runner_ports.rs"]
#[allow(dead_code)] // the example is included for its functions; its main is unused here
mod runner_ports;

use br_agentic_contract::ThreadRef;
use br_llm_messages::{AgentId, Author};
use br_runner_threads::Error;
use br_runner_threads::test_double::MemoryThreads;
use contract_threads::AiDelivery;
use runner_ports::{NodePorts, RunSnapshot, node_ports, thread_of};
use support::adapter_wire::{ai_of, stores};
use support::adapter_x::fault_of;
use support::*;

async fn ports_over_a_double(agent: AgentId) -> (NodePorts, MemoryThreads) {
    let threads = MemoryThreads::new();
    let ports = node_ports(
        &threads.client(),
        agent,
        Author::new("Analyst").expect("author"),
    )
    .await
    .expect("the node ports");
    (ports, threads)
}

#[tokio::test]
async fn the_thread_port_gives_the_thread_reference_of_the_snapshot() {
    let (ports, _threads) = ports_over_a_double(agent_id()).await;
    let thread = v7();

    let found = thread_of(
        &ports,
        &RunSnapshot {
            thread: Some(thread),
        },
    )
    .await
    .expect("a thread reference");

    assert_eq!(found, ThreadRef::from_uuid(thread).expect("thread ref"));
}

#[tokio::test]
async fn the_thread_port_refuses_a_snapshot_without_a_thread_with_an_invalid_input_fault() {
    let (ports, _threads) = ports_over_a_double(agent_id()).await;

    let result = thread_of(&ports, &RunSnapshot { thread: None }).await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
}

#[tokio::test]
async fn the_thread_access_given_to_the_node_writes_an_input_turn_to_threads() {
    let agent = agent_id();
    let (ports, threads) = ports_over_a_double(agent).await;
    let thread = thread_ref();
    let turn = input_turn(vec![notice_entry("the run starts")]);
    let turn_id = turn.id();

    ports
        .thread_access
        .add_input_turn(&thread, turn)
        .await
        .expect("accepted");

    eventually("the input turn is stored", || {
        !threads.commands().is_empty()
    })
    .await;
    let stored = stores(&threads);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].message_id, turn_id.as_uuid());
    assert_eq!(stored[0].thread_id, thread.as_uuid());
    assert_eq!(ai_of(&stored[0]).agent_id, agent.as_uuid());
    assert_eq!(ai_of(&stored[0]).author_name, "Analyst");
    assert!(matches!(
        ai_of(&stored[0]).delivery,
        AiDelivery::Completed { .. }
    ));
}

#[tokio::test]
async fn the_adapter_the_runner_keeps_shares_the_queue_of_the_thread_access_given_to_the_node() {
    let (ports, threads) = ports_over_a_double(agent_id()).await;
    let thread = thread_ref();
    threads.pause();

    ports
        .thread_access
        .add_input_turn(&thread, input_turn(vec![notice_entry("waits")]))
        .await
        .expect("accepted");

    let state = ports.adapter.queue_state();
    assert_eq!(state.threads.len(), 1);
    assert_eq!(state.threads[0].thread, thread);
    assert_eq!(state.threads[0].waiting, 1);
    threads.resume();
    eventually("the queue drains", || {
        ports
            .adapter
            .queue_state()
            .threads
            .iter()
            .all(|t| t.waiting == 0)
    })
    .await;
}
