mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use br_runner_threads::test_double::CommandBody;
use br_runner_threads::{EntryOutcome, Error, MessageStatus};
use support::queue_kit::*;
use support::*;

#[tokio::test]
async fn an_entry_of_a_thread_the_adapter_never_saw_is_stored() {
    let world = world().await;

    let outcome = within(
        "entry_stored",
        world.adapter.entry_stored(&thread_ref(), entry_id()),
    )
    .await;

    assert_eq!(outcome, EntryOutcome::Stored);
}

#[tokio::test]
async fn an_entry_that_no_item_holds_and_none_was_removed_for_is_stored() {
    let world = world().await;
    let thread = thread_ref();
    write_input(&world, &thread).await;
    settled(&world).await;

    let outcome = within(
        "entry_stored",
        world.adapter.entry_stored(&thread, entry_id()),
    )
    .await;

    assert_eq!(outcome, EntryOutcome::Stored);
}

#[tokio::test]
async fn an_entry_of_an_input_turn_is_stored_when_its_whole_message_is_delivered() {
    let world = world().await;
    let thread = thread_ref();
    world.threads.pause();
    let (_, entry) = write_input(&world, &thread).await;

    assert!(pends(world.adapter.entry_stored(&thread, entry)).await);
    world.threads.resume();

    let outcome = within("entry_stored", world.adapter.entry_stored(&thread, entry)).await;
    assert_eq!(outcome, EntryOutcome::Stored);
}

#[tokio::test]
async fn an_entry_chunk_is_stored_when_the_bus_has_taken_it() {
    let world = world().await;
    let thread = thread_ref();
    let entry = notice("n");
    let id = entry.id();
    world.threads.pause();
    world
        .access()
        .add_entry(&thread, turn_ref(world.agent), entry)
        .await
        .expect("accepted");

    assert!(pends(world.adapter.entry_stored(&thread, id)).await);
    world.threads.resume();

    let outcome = within("entry_stored", world.adapter.entry_stored(&thread, id)).await;
    assert_eq!(outcome, EntryOutcome::Stored);
    assert_eq!(world.threads.chunks().len(), 1);
}

#[tokio::test]
async fn the_entry_of_a_model_response_is_stored_when_its_delta_chunks_are_taken() {
    let world = world().await;
    let thread = thread_ref();
    let entry = entry_id();
    world.threads.pause();
    world
        .access()
        .complete_response(
            &thread,
            turn_ref(world.agent),
            response_text(entry, "hello"),
        )
        .await
        .expect("accepted");

    assert!(pends(world.adapter.entry_stored(&thread, entry)).await);
    world.threads.resume();

    let outcome = within("entry_stored", world.adapter.entry_stored(&thread, entry)).await;
    assert_eq!(outcome, EntryOutcome::Stored);
    assert!(world.threads.chunks().len() >= 3);
}

#[tokio::test]
async fn an_opening_holds_no_entry() {
    let world = world().await;
    let thread = thread_ref();
    let entry = entry_id();
    world.threads.pause();
    let sink = world
        .access()
        .open_response(&thread, turn_ref(world.agent), entry)
        .await
        .expect("accepted");
    drop(sink);
    assert_eq!(world.adapter.queue_state().threads[0].waiting, 1);

    let outcome = within("entry_stored", world.adapter.entry_stored(&thread, entry)).await;

    assert_eq!(outcome, EntryOutcome::Stored);
}

#[tokio::test]
async fn a_closing_holds_no_entry() {
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
    settled(&world).await;
    world.threads.pause();
    world
        .access()
        .close_turn(&thread, turn, closure)
        .await
        .expect("accepted");
    assert_eq!(world.adapter.queue_state().threads[0].waiting, 1);

    let outcome = within(
        "entry_stored",
        world.adapter.entry_stored(&thread, closure_entry),
    )
    .await;

    assert_eq!(outcome, EntryOutcome::Stored);
}

#[tokio::test]
async fn an_entry_whose_chunk_was_removed_after_a_report_is_dropped_now_and_later() {
    let world = world().await;
    let thread = thread_ref();
    world
        .threads
        .answer_chunks(|_, _, _| Err(Error::InvalidInput("refused".to_owned())));
    let entry = notice("n");
    let id = entry.id();
    world
        .access()
        .add_entry(&thread, turn_ref(world.agent), entry)
        .await
        .expect("accepted");

    let now = within("entry_stored", world.adapter.entry_stored(&thread, id)).await;
    settled(&world).await;
    let later = within("entry_stored", world.adapter.entry_stored(&thread, id)).await;

    assert_eq!(now, EntryOutcome::Dropped);
    assert_eq!(later, EntryOutcome::Dropped);
}

#[tokio::test]
async fn an_entry_whose_whole_message_was_refused_is_dropped() {
    let world = world().await;
    let thread = thread_ref();
    world
        .threads
        .answer_commands(|_| Err(Error::InvalidInput("refused".to_owned())));

    let (_, entry) = write_input(&world, &thread).await;

    let outcome = within("entry_stored", world.adapter.entry_stored(&thread, entry)).await;
    assert_eq!(outcome, EntryOutcome::Dropped);
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

    let outcome = within("entry_stored", world.adapter.entry_stored(&thread, id)).await;

    assert_eq!(outcome, EntryOutcome::Dropped);
    assert!(world.threads.chunks().is_empty());
}

#[tokio::test]
async fn a_new_item_of_a_dropped_entry_clears_the_record() {
    let world = world().await;
    let thread = thread_ref();
    let refusing = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&refusing);
    world.threads.answer_commands(move |command| {
        if flag.load(Ordering::SeqCst) {
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
    let dropped = within("entry_stored", world.adapter.entry_stored(&thread, id)).await;
    refusing.store(false, Ordering::SeqCst);

    world
        .access()
        .add_input_turn(&thread, input_turn(vec![entry]))
        .await
        .expect("accepted");

    let outcome = within("entry_stored", world.adapter.entry_stored(&thread, id)).await;
    assert_eq!(dropped, EntryOutcome::Dropped);
    assert_eq!(outcome, EntryOutcome::Stored);
}
