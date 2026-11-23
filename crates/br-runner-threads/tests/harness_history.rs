mod support;

use std::sync::Arc;

use br_agentic_contract::{PortError, ThreadAccess};
use br_agentic_harness_contract::{HarnessHistory, StoredAnswer};
use br_runner_threads::test_double::Outcome;
use br_runner_threads::{EntryOutcome, Error, MessageStatus, ThreadAccessAdapter};
use support::adapter_wire::*;
use support::queue_kit::*;
use support::*;

pub async fn stored_of(
    world: &World,
    thread: &br_agentic_contract::ThreadRef,
    entry: br_llm_messages::EntryId,
) -> Result<StoredAnswer, PortError> {
    within("stored", history(&world.adapter).stored(thread, entry)).await
}

pub fn history(adapter: &Arc<ThreadAccessAdapter>) -> &dyn HarnessHistory {
    adapter.as_ref()
}

#[tokio::test]
async fn one_adapter_value_is_the_thread_access_of_a_node_and_the_history_port_of_the_harness() {
    let world = world().await;
    let thread = thread_ref();
    let as_history: &dyn HarnessHistory = world.adapter.as_ref();
    let as_access: &dyn ThreadAccess = world.adapter.as_ref();
    let upcast: &dyn ThreadAccess = as_history;

    let held = small_conversation(thread.as_uuid());
    let answered = held.clone();
    world
        .threads
        .answer_graphql(move |_document, _variables| Ok(context_answer(thread, &answered)));
    let (_, entry) = write_input(&world, &thread).await;
    let conversation = within("read", as_access.read(&thread)).await;
    let answer = within("stored", as_history.stored(&thread, entry)).await;
    let through_upcast = within("read", upcast.read(&thread)).await;

    assert_eq!(conversation.expect("a conversation"), held);
    assert_eq!(through_upcast.expect("a conversation"), held);
    assert_eq!(answer.expect("known"), StoredAnswer::Stored);
    assert_eq!(
        world.adapter.entry_stored(&thread, entry).await,
        EntryOutcome::Stored
    );
}

#[tokio::test]
async fn an_entry_of_a_thread_the_adapter_never_saw_is_a_fault_of_the_port() {
    let world = world().await;

    let error = stored_of(&world, &thread_ref(), entry_id())
        .await
        .expect_err("a fault");

    assert!(matches!(fault_of(error), Error::UnknownEntry(_)));
}

#[tokio::test]
async fn an_entry_that_no_item_holds_and_none_was_removed_for_is_a_fault_of_the_port() {
    let world = world().await;
    let thread = thread_ref();
    write_input(&world, &thread).await;
    settled(&world).await;
    let stranger = entry_id();

    let error = stored_of(&world, &thread, stranger)
        .await
        .expect_err("a fault");

    assert!(matches!(fault_of(error), Error::UnknownEntry(_)));
    assert_eq!(
        world.adapter.entry_stored(&thread, stranger).await,
        EntryOutcome::Stored
    );
}

#[tokio::test]
async fn an_opening_and_a_closing_hold_no_entry_for_the_report() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    let closure =
        br_llm_messages::TurnClosure::new(entry_id(), br_llm_messages::ClosureReason::completed());
    let closure_entry = closure.id();
    world
        .access()
        .add_entry(&thread, turn.clone(), notice("n"))
        .await
        .expect("accepted");
    world
        .access()
        .close_turn(&thread, turn, closure)
        .await
        .expect("accepted");
    settled(&world).await;

    let error = stored_of(&world, &thread, closure_entry)
        .await
        .expect_err("a fault");

    assert!(matches!(fault_of(error), Error::UnknownEntry(_)));
}

#[tokio::test]
async fn a_whole_message_is_stored_only_when_its_command_is_confirmed() {
    let world = world().await;
    let thread = thread_ref();
    world.threads.pause();
    let (_, entry) = write_input(&world, &thread).await;

    assert!(pends(history(&world.adapter).stored(&thread, entry)).await);
    world.threads.resume();

    assert_eq!(
        stored_of(&world, &thread, entry).await.expect("known"),
        StoredAnswer::Stored
    );
}

#[tokio::test]
async fn a_whole_message_that_threads_confirmed_as_failed_is_dropped() {
    let world = world().await;
    let thread = thread_ref();
    world.threads.answer_commands(|command| {
        Ok(Outcome {
            command_id: command.id,
            status: Some(MessageStatus::Failed),
        })
    });

    let (_, entry) = write_input(&world, &thread).await;

    assert_eq!(
        stored_of(&world, &thread, entry).await.expect("known"),
        StoredAnswer::Dropped
    );
}

#[tokio::test]
async fn a_whole_message_that_threads_refused_is_dropped() {
    let world = world().await;
    let thread = thread_ref();
    world
        .threads
        .answer_commands(|_| Err(Error::InvalidInput("refused".to_owned())));

    let (_, entry) = write_input(&world, &thread).await;

    assert_eq!(
        stored_of(&world, &thread, entry).await.expect("known"),
        StoredAnswer::Dropped
    );
}

#[tokio::test]
async fn a_new_item_of_a_dropped_entry_clears_the_drop() {
    let world = world().await;
    let thread = thread_ref();
    let refusing = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = Arc::clone(&refusing);
    world.threads.answer_commands(move |command| {
        if flag.load(std::sync::atomic::Ordering::SeqCst) {
            Err(Error::InvalidInput("refused".to_owned()))
        } else {
            Ok(taken(command, MessageStatus::Completed))
        }
    });
    let entry = human_entry("again");
    let first = input_turn(vec![entry.clone()]);
    let id = first.entries()[0].id();
    world
        .access()
        .add_input_turn(&thread, first)
        .await
        .expect("accepted");
    let dropped = stored_of(&world, &thread, id).await.expect("known");
    refusing.store(false, std::sync::atomic::Ordering::SeqCst);

    world
        .access()
        .add_input_turn(&thread, input_turn(vec![entry]))
        .await
        .expect("accepted");

    assert_eq!(dropped, StoredAnswer::Dropped);
    assert_eq!(
        stored_of(&world, &thread, id).await.expect("known"),
        StoredAnswer::Stored
    );
}

#[tokio::test]
async fn an_entry_kept_in_the_store_by_an_earlier_adapter_is_known_to_the_next_one() {
    let world = world().await;
    let thread = thread_ref();
    world.threads.pause();
    let (_, entry) = write_input(&world, &thread).await;
    let World {
        threads,
        client,
        store,
        recorder,
        adapter,
        agent,
    } = world;
    drop(adapter);
    let next = ThreadAccessAdapter::new(&client, settings(store, recorder, agent))
        .await
        .expect("adapter");
    let next = Arc::new(next);

    assert!(pends(history(&next).stored(&thread, entry)).await);
    threads.resume();

    let answer = within("stored", history(&next).stored(&thread, entry)).await;
    assert_eq!(answer.expect("known"), StoredAnswer::Stored);
}
