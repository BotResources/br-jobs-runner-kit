mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use br_llm_messages::{ClosureReason, TurnClosure};
use br_runner_threads::{ItemKind, MessageStatus, QueueStore, ReportKind};
use support::queue_kit::*;
use support::*;

#[tokio::test]
async fn the_items_of_one_thread_reach_threads_in_the_order_they_were_accepted() {
    let world = world().await;
    let thread = thread_ref();
    let mut accepted = Vec::new();
    for _ in 0..5 {
        accepted.push(write_input(&world, &thread).await.0);
    }

    eventually("all five delivered", || world.threads.commands().len() == 5).await;

    assert_eq!(command_messages(&world.threads), accepted);
}

#[tokio::test]
async fn the_items_of_a_thread_go_to_threads_one_at_a_time() {
    let world = world().await;
    let thread = thread_ref();
    world.threads.pause();
    let mut accepted = Vec::new();
    for _ in 0..3 {
        accepted.push(write_input(&world, &thread).await.0);
    }

    eventually("the first item was attempted", || {
        world.threads.operations().len() == 1
    })
    .await;
    quiet().await;
    assert_eq!(world.threads.operations().len(), 1);
    world.threads.resume();

    eventually("all three delivered", || {
        world.threads.commands().len() == 3
    })
    .await;
    assert_eq!(command_messages(&world.threads), accepted);
}

#[tokio::test]
async fn an_item_that_cannot_be_delivered_blocks_every_later_item_of_its_thread() {
    let world = world().await;
    let thread = thread_ref();
    let stuck = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&stuck);
    world.threads.answer_commands(move |command| {
        if flag.load(Ordering::SeqCst) {
            Err(unavailable())
        } else {
            Ok(taken(command, MessageStatus::Completed))
        }
    });
    let (first, _) = write_input(&world, &thread).await;
    let (second, _) = write_input(&world, &thread).await;

    eventually("the first item is attempted again and again", || {
        world.threads.commands().len() >= 3
    })
    .await;
    assert!(
        command_messages(&world.threads)
            .iter()
            .all(|message| *message == first)
    );
    stuck.store(false, Ordering::SeqCst);

    eventually("the second item is delivered", || {
        command_messages(&world.threads).contains(&second)
    })
    .await;
    let mut sent = command_messages(&world.threads);
    sent.dedup();
    assert_eq!(sent, vec![first, second]);
}

#[tokio::test]
async fn an_item_that_cannot_be_delivered_does_not_block_another_thread() {
    let world = world().await;
    let (stuck_thread, free_thread) = (thread_ref(), thread_ref());
    let stuck = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&stuck);
    let stuck_uuid = stuck_thread.as_uuid();
    world.threads.answer_commands(move |command| {
        if thread_of(command) == stuck_uuid && flag.load(Ordering::SeqCst) {
            Err(unavailable())
        } else {
            Ok(taken(command, MessageStatus::Completed))
        }
    });
    let (stuck_message, _) = write_input(&world, &stuck_thread).await;
    let (free_message, _) = write_input(&world, &free_thread).await;

    eventually("only the stuck thread still waits", || {
        let state = world.adapter.queue_state();
        state.threads.len() == 1 && state.threads[0].thread == stuck_thread
    })
    .await;
    assert!(command_messages(&world.threads).contains(&free_message));
    assert_eq!(world.adapter.queue_state().threads[0].waiting, 1);
    stuck.store(false, Ordering::SeqCst);

    settled(&world).await;
    assert!(command_messages(&world.threads).contains(&stuck_message));
}

#[tokio::test]
async fn no_write_operation_waits_for_threads() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    let discarded = entry_id();
    world.threads.pause();
    let access = world.access();

    let input = input_turn(vec![human_entry("hi")]);
    within("add_input_turn", access.add_input_turn(&thread, input))
        .await
        .expect("accepted");
    within(
        "add_entry",
        access.add_entry(&thread, turn.clone(), notice("n")),
    )
    .await
    .expect("accepted");
    let sink = within(
        "open_response",
        access.open_response(&thread, turn.clone(), discarded),
    )
    .await
    .expect("accepted");
    drop(sink);
    within(
        "discard_response",
        access.discard_response(&thread, discarded),
    )
    .await
    .expect("accepted");
    within(
        "complete_response",
        access.complete_response(&thread, turn.clone(), response_text(entry_id(), "hi")),
    )
    .await
    .expect("accepted");
    let result = within(
        "open_result",
        access.open_result(&thread, turn.clone(), entry_id()),
    )
    .await
    .expect("accepted");
    drop(result);
    let results = tool_results(entry_id(), block_id(), "tool", "ok");
    within(
        "complete_result",
        access.complete_result(&thread, turn.clone(), results),
    )
    .await
    .expect("accepted");
    let closure = TurnClosure::new(entry_id(), ClosureReason::completed());
    within("close_turn", access.close_turn(&thread, turn, closure))
        .await
        .expect("accepted");

    eventually("the head was attempted", || {
        world.threads.operations().len() == 1
    })
    .await;
    quiet().await;
    assert_eq!(world.threads.operations().len(), 1);
    assert!(world.adapter.queue_state().threads[0].waiting > 1);
    world.threads.resume();
    settled(&world).await;
    assert_eq!(world.threads.commands().len(), 3);
}

#[tokio::test]
async fn dropping_the_adapter_reports_every_item_left_as_unsent_and_keeps_it_in_the_store() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    world.threads.pause();
    let (first, _) = write_input(&world, &thread).await;
    world
        .access()
        .add_entry(&thread, turn.clone(), notice("n"))
        .await
        .expect("accepted");
    let (recorder, store) = (Arc::clone(&world.recorder), Arc::clone(&world.store));

    drop(world);

    eventually("three reports", || recorder.reports().len() == 3).await;
    let reports = recorder.reports();
    assert!(reports.iter().all(|r| r.kind == ReportKind::Unsent));
    assert!(reports.iter().all(|r| r.thread == thread));
    assert!(reports.iter().all(|r| r.error.is_none()));
    let kinds: Vec<ItemKind> = reports.iter().map(|r| r.item_kind).collect();
    assert_eq!(
        kinds,
        vec![ItemKind::WholeMessage, ItemKind::Opening, ItemKind::Chunk]
    );
    let messages: Vec<_> = reports.iter().map(|r| r.message_id).collect();
    assert_eq!(messages, vec![first, turn.id.as_uuid(), turn.id.as_uuid()]);
    assert_eq!(store.list().await.expect("list").len(), 3);
}
