mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use br_agentic_contract::ThreadAccess;
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{Error, MessageStatus};
use support::queue_kit::*;
use support::queue_store::*;
use support::*;

async fn write_notice(
    world: &World,
    thread: &br_agentic_contract::ThreadRef,
    agent: br_llm_messages::AgentId,
) -> br_agentic_contract::AiTurnRef {
    let turn = turn_ref(agent);
    world
        .access()
        .add_entry(thread, turn.clone(), notice("n"))
        .await
        .expect("accepted");
    turn
}

fn refuse_the_stuck_message(world: &World, stuck: &Arc<AtomicBool>, message: uuid::Uuid) {
    let flag = Arc::clone(stuck);
    world.threads.answer_commands(move |command| {
        if flag.load(Ordering::SeqCst) && message_of(command) == message {
            Err(unavailable())
        } else {
            Ok(taken(command, MessageStatus::Streaming))
        }
    });
}

#[tokio::test]
async fn ending_an_instance_that_holds_no_item_and_no_state_returns_at_once() {
    let world = world().await;
    write_input(&world, &thread_ref()).await;

    within("instance_ended", world.adapter.instance_ended(agent_id())).await;
}

#[tokio::test]
async fn ending_an_instance_waits_until_its_items_accepted_before_the_call_have_ended() {
    let world = world().await;
    let agent = agent_id();
    world.threads.pause();
    write_notice(&world, &thread_ref(), agent).await;

    assert!(pends(world.adapter.instance_ended(agent)).await);
    world.threads.resume();

    within("instance_ended", world.adapter.instance_ended(agent)).await;
    assert!(world.adapter.queue_state().threads.is_empty());
}

#[tokio::test]
async fn ending_an_instance_also_waits_for_the_items_of_other_agents_on_its_threads() {
    let world = world().await;
    let thread = thread_ref();
    let (agent, other) = (agent_id(), agent_id());
    let stuck = Arc::new(AtomicBool::new(true));
    let other_turn = turn_ref(other);
    refuse_the_stuck_message(&world, &stuck, other_turn.id.as_uuid());
    write_notice(&world, &thread, agent).await;
    world
        .access()
        .add_entry(&thread, other_turn, notice("other"))
        .await
        .expect("accepted");
    eventually("only the item of the other agent waits", || {
        world
            .adapter
            .queue_state()
            .threads
            .first()
            .is_some_and(|t| t.waiting == 2 && t.last_failure.is_some())
    })
    .await;

    assert!(pends(world.adapter.instance_ended(agent)).await);
    stuck.store(false, Ordering::SeqCst);

    within("instance_ended", world.adapter.instance_ended(agent)).await;
}

#[tokio::test]
async fn ending_an_instance_does_not_wait_for_items_accepted_after_the_call() {
    let world = world().await;
    let thread = thread_ref();
    let (agent, other) = (agent_id(), agent_id());
    let stuck = Arc::new(AtomicBool::new(true));
    let other_turn = turn_ref(other);
    refuse_the_stuck_message(&world, &stuck, other_turn.id.as_uuid());
    world.threads.pause();
    write_notice(&world, &thread, agent).await;
    let mut ended = Box::pin(world.adapter.instance_ended(agent));
    assert!(futures::poll!(ended.as_mut()).is_pending());

    world
        .access()
        .add_entry(&thread, other_turn, notice("later"))
        .await
        .expect("accepted");
    world.threads.resume();

    within("instance_ended", ended).await;
    assert!(!world.adapter.queue_state().threads.is_empty());
}

#[tokio::test]
async fn the_items_left_in_a_store_by_an_earlier_adapter_count_for_their_agent() {
    let store = FlakyStore::new();
    let recorder = Arc::new(Recorder::default());
    let agent = agent_id();
    let earlier = MemoryThreads::new();
    earlier.pause();
    let first = build_adapter(
        &earlier.client(),
        store.clone(),
        recorder.clone(),
        agent_id(),
    )
    .await
    .expect("adapter");
    first
        .add_entry(&thread_ref(), turn_ref(agent), notice("n"))
        .await
        .expect("accepted");
    drop(first);
    assert_eq!(store.kept().await.len(), 2);

    let threads = MemoryThreads::new();
    threads.pause();
    let adapter = build_adapter(&threads.client(), store.clone(), recorder, agent_id())
        .await
        .expect("adapter");

    assert!(pends(adapter.instance_ended(agent)).await);
    threads.resume();
    within("instance_ended", adapter.instance_ended(agent)).await;
    settled_over(&adapter, store.as_ref()).await;
}

#[tokio::test]
async fn ending_an_instance_queues_nothing() {
    let world = world().await;
    let agent = agent_id();
    let turn = write_notice(&world, &thread_ref(), agent).await;
    settled(&world).await;
    let before = world.threads.operations();

    within("instance_ended", world.adapter.instance_ended(agent)).await;
    quiet().await;

    assert_eq!(world.threads.operations(), before);
    assert!(world.adapter.queue_state().threads.is_empty());
    assert_eq!(command_messages(&world.threads), vec![turn.id.as_uuid()]);
}

#[tokio::test]
async fn after_the_end_of_an_instance_its_turn_has_no_state_so_a_completed_closing_is_refused() {
    let world = world().await;
    let thread = thread_ref();
    let agent = agent_id();
    let turn = write_notice(&world, &thread, agent).await;
    let closure = || {
        br_llm_messages::TurnClosure::new(entry_id(), br_llm_messages::ClosureReason::completed())
    };

    within("instance_ended", world.adapter.instance_ended(agent)).await;
    let refused = world
        .access()
        .close_turn(&thread, turn.clone(), closure())
        .await
        .expect_err("refused");

    assert!(matches!(fault_of(refused), Error::InvalidInput(_)));
    let other = turn_ref(agent_id());
    world
        .access()
        .add_entry(&thread, other.clone(), notice("kept"))
        .await
        .expect("accepted");
    world
        .access()
        .close_turn(&thread, other, closure())
        .await
        .expect("a turn of another agent still has its state");
}

#[tokio::test]
async fn after_the_end_of_an_instance_a_new_entry_for_its_turn_is_queued_as_an_opening_again() {
    let world = world().await;
    let thread = thread_ref();
    let (agent, other) = (agent_id(), agent_id());
    let ended_turn = write_notice(&world, &thread, agent).await;
    let kept_turn = write_notice(&world, &thread, other).await;
    settled(&world).await;
    assert_eq!(world.threads.commands().len(), 2);

    within("instance_ended", world.adapter.instance_ended(agent)).await;
    for turn in [&ended_turn, &kept_turn] {
        world
            .access()
            .add_entry(&thread, turn.clone(), notice("again"))
            .await
            .expect("accepted");
    }
    settled(&world).await;

    let messages = command_messages(&world.threads);
    let openings_of = |turn: &br_agentic_contract::AiTurnRef| {
        messages.iter().filter(|m| **m == turn.id.as_uuid()).count()
    };
    assert_eq!(openings_of(&ended_turn), 2);
    assert_eq!(openings_of(&kept_turn), 1);
    let seqs_of = |turn: &br_agentic_contract::AiTurnRef| -> Vec<u64> {
        world
            .threads
            .chunks()
            .iter()
            .filter(|c| c.0 == turn.id.as_uuid())
            .map(|c| c.1)
            .collect()
    };
    assert_eq!(seqs_of(&ended_turn), vec![0, 0]);
    assert_eq!(seqs_of(&kept_turn), vec![0, 1]);
}
