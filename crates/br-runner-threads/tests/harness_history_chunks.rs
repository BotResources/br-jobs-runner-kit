mod support;

use br_agentic_harness_contract::{HarnessHistory, StoredAnswer};
use br_runner_threads::test_double::{CommandBody, Outcome};
use br_runner_threads::{Error, MessageStatus};
use support::queue_kit::*;
use support::*;

async fn stored(
    world: &World,
    thread: &br_agentic_contract::ThreadRef,
    entry: br_llm_messages::EntryId,
) -> StoredAnswer {
    let port: &dyn HarnessHistory = world.adapter.as_ref();
    within("stored", port.stored(thread, entry))
        .await
        .expect("known")
}

async fn pending(
    world: &World,
    thread: &br_agentic_contract::ThreadRef,
    entry: br_llm_messages::EntryId,
) -> bool {
    let port: &dyn HarnessHistory = world.adapter.as_ref();
    pends(port.stored(thread, entry)).await
}

fn closure() -> br_llm_messages::TurnClosure {
    br_llm_messages::TurnClosure::new(entry_id(), br_llm_messages::ClosureReason::completed())
}

#[tokio::test]
async fn a_chunk_taken_by_the_bus_is_not_stored_until_the_closing_is_confirmed() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    let entry = notice("n");
    let id = entry.id();
    world
        .access()
        .add_entry(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    eventually("the chunk is taken", || world.threads.chunks().len() == 1).await;

    assert!(pending(&world, &thread, id).await);
    world.threads.pause();
    world
        .access()
        .close_turn(&thread, turn, closure())
        .await
        .expect("accepted");
    assert!(pending(&world, &thread, id).await);
    world.threads.resume();

    assert_eq!(stored(&world, &thread, id).await, StoredAnswer::Stored);
}

#[tokio::test]
async fn the_entry_of_a_streamed_response_is_stored_after_the_closing() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    let id = entry_id();
    world
        .access()
        .complete_response(&thread, turn.clone(), response_text(id, "hello"))
        .await
        .expect("accepted");
    eventually("the chunks are taken", || world.threads.chunks().len() >= 3).await;
    assert!(pending(&world, &thread, id).await);

    world
        .access()
        .close_turn(&thread, turn, closure())
        .await
        .expect("accepted");

    assert_eq!(stored(&world, &thread, id).await, StoredAnswer::Stored);
}

#[tokio::test]
async fn a_closing_confirmed_as_failed_drops_the_entries_of_the_turn() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    world.threads.answer_commands(|command| match command.body {
        CommandBody::Store(_) => Ok(taken(command, MessageStatus::Completed)),
        _ => Ok(Outcome {
            command_id: command.id,
            status: Some(MessageStatus::Failed),
        }),
    });
    let entry = notice("n");
    let id = entry.id();
    world
        .access()
        .add_entry(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    world
        .access()
        .close_turn(&thread, turn, closure())
        .await
        .expect("accepted");

    assert_eq!(stored(&world, &thread, id).await, StoredAnswer::Dropped);
}

#[tokio::test]
async fn a_refused_closing_drops_the_entries_of_the_turn() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    world.threads.answer_commands(|command| match command.body {
        CommandBody::Store(_) => Ok(taken(command, MessageStatus::Completed)),
        _ => Err(Error::InvalidInput("refused".to_owned())),
    });
    let entry = notice("n");
    let id = entry.id();
    world
        .access()
        .add_entry(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    world
        .access()
        .close_turn(&thread, turn, closure())
        .await
        .expect("accepted");

    assert_eq!(stored(&world, &thread, id).await, StoredAnswer::Dropped);
}

#[tokio::test]
async fn a_chunk_that_was_removed_after_a_report_is_dropped_whatever_the_closing_says() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    world
        .threads
        .answer_chunks(|_, _, _| Err(Error::InvalidInput("refused".to_owned())));
    let entry = notice("n");
    let id = entry.id();
    world
        .access()
        .add_entry(&thread, turn.clone(), entry)
        .await
        .expect("accepted");

    assert_eq!(stored(&world, &thread, id).await, StoredAnswer::Dropped);
    let _ = world.access().close_turn(&thread, turn, closure()).await;
    settled(&world).await;
    assert_eq!(stored(&world, &thread, id).await, StoredAnswer::Dropped);
}

#[tokio::test]
async fn an_entry_removed_unsent_after_a_refused_opening_is_dropped() {
    let world = world().await;
    let thread = thread_ref();
    world.threads.answer_commands(|command| match command.body {
        CommandBody::Store(_) => Err(Error::InvalidInput("refused".to_owned())),
        _ => Ok(taken(command, MessageStatus::Completed)),
    });
    let entry = notice("n");
    let id = entry.id();
    world
        .access()
        .add_entry(&thread, turn_ref(world.agent), entry)
        .await
        .expect("accepted");

    assert_eq!(stored(&world, &thread, id).await, StoredAnswer::Dropped);
}

#[tokio::test]
async fn a_turn_that_is_never_closed_leaves_the_report_waiting() {
    let world = world().await;
    let thread = thread_ref();
    let entry = notice("n");
    let id = entry.id();
    world
        .access()
        .add_entry(&thread, turn_ref(world.agent), entry)
        .await
        .expect("accepted");
    settled(&world).await;

    assert!(pending(&world, &thread, id).await);
}
