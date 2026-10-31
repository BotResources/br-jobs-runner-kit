mod support;

use std::time::Instant;

use br_runner_threads::test_double::CommandBody;
use br_runner_threads::{ItemKind, MessageStatus};
use support::queue_kit::*;
use support::*;

#[tokio::test]
async fn the_state_of_an_idle_adapter_names_no_thread() {
    let world = world().await;

    assert!(world.adapter.queue_state().threads.is_empty());
}

#[tokio::test]
async fn the_state_gives_for_each_waiting_thread_the_count_and_the_oldest_item() {
    let world = world().await;
    let (busy, quiet_thread) = (thread_ref(), thread_ref());
    world.threads.pause();
    let before = Instant::now();
    let (busy_first, _) = write_input(&world, &busy).await;
    write_input(&world, &busy).await;
    let turn = turn_ref(world.agent);
    world
        .access()
        .add_entry(&quiet_thread, turn.clone(), notice("n"))
        .await
        .expect("accepted");
    let after = Instant::now();

    let state = world.adapter.queue_state();

    assert_eq!(state.threads.len(), 2);
    let busy_state = state
        .threads
        .iter()
        .find(|t| t.thread == busy)
        .expect("busy");
    assert_eq!(busy_state.waiting, 2);
    let oldest = busy_state.oldest.as_ref().expect("an oldest item");
    assert_eq!(oldest.message_id, busy_first);
    assert_eq!(oldest.kind, ItemKind::WholeMessage);
    assert!(oldest.accepted_at >= before && oldest.accepted_at <= after);
    assert!(busy_state.last_failure.is_none());
    let other = state
        .threads
        .iter()
        .find(|t| t.thread == quiet_thread)
        .expect("other");
    assert_eq!(other.waiting, 2);
    let oldest = other.oldest.as_ref().expect("an oldest item");
    assert_eq!(oldest.message_id, turn.id.as_uuid());
    assert_eq!(oldest.kind, ItemKind::Opening);
}

#[tokio::test]
async fn the_state_gives_the_last_failed_attempt_until_an_item_is_delivered() {
    let world = world().await;
    let thread = thread_ref();
    world.threads.answer_commands(|_| Err(unavailable()));
    let (first, _) = write_input(&world, &thread).await;
    let (second, _) = write_input(&world, &thread).await;

    eventually("a failed attempt is shown", || {
        world
            .adapter
            .queue_state()
            .threads
            .first()
            .is_some_and(|t| t.last_failure.is_some())
    })
    .await;
    let state = world.adapter.queue_state();
    assert_eq!(state.threads[0].waiting, 2);
    let failure = state.threads[0].last_failure.clone().expect("a failure");
    assert_eq!(failure.error, unavailable());
    assert_eq!(failure.item_id, world.threads.commands()[0].id);

    let holder = world.threads.clone();
    world.threads.answer_commands(move |command| {
        holder.pause();
        Ok(taken(command, MessageStatus::Completed))
    });
    eventually("the first item is delivered and the second waits", || {
        let state = world.adapter.queue_state();
        state.threads.len() == 1
            && state.threads[0].waiting == 1
            && state.threads[0]
                .oldest
                .as_ref()
                .is_some_and(|o| o.message_id == second)
    })
    .await;
    let state = world.adapter.queue_state();
    assert!(state.threads[0].last_failure.is_none());
    assert!(command_messages(&world.threads).contains(&first));
    world.threads.resume();
}

#[tokio::test]
async fn the_state_of_a_stuck_thread_is_read_without_waiting_for_threads() {
    let world = world().await;
    let thread = thread_ref();
    world.threads.pause();
    write_input(&world, &thread).await;
    eventually("the head was attempted", || {
        world
            .threads
            .commands()
            .iter()
            .any(|c| matches!(c.body, CommandBody::Store(_)))
    })
    .await;

    let state = world.adapter.queue_state();

    assert_eq!(state.threads[0].waiting, 1);
}
