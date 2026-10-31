mod support;

use std::sync::Arc;

use br_agentic_contract::AiTurnRef;
use br_llm_messages::{ClosureReason, TurnClosure};
use br_runner_threads::test_double::{CommandBody, MemoryThreads};
use br_runner_threads::{Error, ItemKind, MemoryQueueStore, MessageStatus, ReportKind};
use support::queue_kit::*;
use support::queue_store::*;
use support::*;

fn refuse_the_opening_of(threads: &MemoryThreads, turn: &AiTurnRef) {
    let message = turn.id.as_uuid();
    threads.answer_commands(move |command| {
        let opening = matches!(command.body, CommandBody::Store(_));
        if opening && message_of(command) == message {
            Err(Error::InvalidInput("the opening is refused".to_owned()))
        } else {
            Ok(taken(command, MessageStatus::Completed))
        }
    });
}

async fn complete_closing(
    world: &World,
    thread: &br_agentic_contract::ThreadRef,
    turn: &AiTurnRef,
) {
    let closure = TurnClosure::new(entry_id(), ClosureReason::completed());
    world
        .access()
        .close_turn(thread, turn.clone(), closure)
        .await
        .expect("accepted");
}

#[tokio::test]
async fn a_refused_opening_removes_the_later_items_of_its_message_unsent_and_reports_each() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    refuse_the_opening_of(&world.threads, &turn);
    world.threads.pause();
    for text in ["a", "b"] {
        world
            .access()
            .add_entry(&thread, turn.clone(), notice(text))
            .await
            .expect("accepted");
    }
    complete_closing(&world, &thread, &turn).await;
    let (other, _) = write_input(&world, &thread).await;
    world.threads.resume();

    settled(&world).await;

    eventually("four reports", || world.recorder.reports().len() == 4).await;
    let reports = world.recorder.reports();
    let shape: Vec<_> = reports.iter().map(|r| (r.kind, r.item_kind)).collect();
    assert_eq!(
        shape,
        vec![
            (ReportKind::ItemRefused, ItemKind::Opening),
            (ReportKind::ItemDroppedUnsent, ItemKind::Chunk),
            (ReportKind::ItemDroppedUnsent, ItemKind::Chunk),
            (ReportKind::ItemDroppedUnsent, ItemKind::Closing),
        ]
    );
    assert!(reports.iter().all(|r| r.message_id == turn.id.as_uuid()));
    assert!(reports.iter().all(|r| r.thread == thread));
    assert!(matches!(reports[0].error, Some(Error::InvalidInput(_))));
    assert!(reports[1..].iter().all(|r| r.error.is_none()));
    assert!(world.threads.chunks().is_empty());
    assert_eq!(
        command_messages(&world.threads),
        vec![turn.id.as_uuid(), other]
    );
}

#[tokio::test]
async fn the_items_accepted_after_the_refusal_of_the_opening_are_removed_unsent_too() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    refuse_the_opening_of(&world.threads, &turn);
    world
        .access()
        .add_entry(&thread, turn.clone(), notice("a"))
        .await
        .expect("accepted");
    eventually("the opening is refused", || {
        !reports_of(&world, ReportKind::ItemRefused).is_empty()
    })
    .await;

    world
        .access()
        .add_entry(&thread, turn.clone(), notice("b"))
        .await
        .expect("accepted");
    complete_closing(&world, &thread, &turn).await;

    settled(&world).await;
    eventually("three items dropped unsent", || {
        reports_of(&world, ReportKind::ItemDroppedUnsent).len() == 3
    })
    .await;
    let kinds: Vec<_> = reports_of(&world, ReportKind::ItemDroppedUnsent)
        .iter()
        .map(|r| r.item_kind)
        .collect();
    assert_eq!(
        kinds,
        vec![ItemKind::Chunk, ItemKind::Chunk, ItemKind::Closing]
    );
    assert!(world.threads.chunks().is_empty());
    assert_eq!(world.threads.commands().len(), 1);
}

#[tokio::test]
async fn a_refused_closing_is_reported_as_closing_failed_and_removed() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    world.threads.answer_commands(|command| {
        if matches!(command.body, CommandBody::Complete(_)) {
            Err(Error::Unauthenticated)
        } else {
            Ok(taken(command, MessageStatus::Streaming))
        }
    });
    world
        .access()
        .add_entry(&thread, turn.clone(), notice("a"))
        .await
        .expect("accepted");
    complete_closing(&world, &thread, &turn).await;
    let (later, _) = write_input(&world, &thread).await;

    settled(&world).await;

    eventually("the failure is reported", || {
        !reports_of(&world, ReportKind::ClosingFailed).is_empty()
    })
    .await;
    let failed = reports_of(&world, ReportKind::ClosingFailed);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].item_kind, ItemKind::Closing);
    assert_eq!(failed[0].error, Some(Error::Unauthenticated));
    assert_eq!(failed[0].message_id, turn.id.as_uuid());
    assert!(reports_of(&world, ReportKind::ItemRefused).is_empty());
    assert!(command_messages(&world.threads).contains(&later));
}

#[tokio::test]
async fn a_closing_that_threads_confirms_as_failed_is_reported_without_an_error() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    world.threads.answer_commands(|command| {
        let status = if matches!(command.body, CommandBody::Complete(_)) {
            MessageStatus::Failed
        } else {
            MessageStatus::Streaming
        };
        Ok(taken(command, status))
    });
    world
        .access()
        .add_entry(&thread, turn.clone(), notice("a"))
        .await
        .expect("accepted");
    complete_closing(&world, &thread, &turn).await;

    settled(&world).await;

    eventually("the failure is reported", || {
        !reports_of(&world, ReportKind::ClosingFailed).is_empty()
    })
    .await;
    let failed = reports_of(&world, ReportKind::ClosingFailed);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].item_kind, ItemKind::Closing);
    assert_eq!(failed[0].error, None);
    assert_eq!(failed[0].message_id, turn.id.as_uuid());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_report_that_the_observer_has_not_taken_does_not_hold_delivery() {
    let threads = MemoryThreads::new();
    let observer = GatedObserver::closed();
    let store = Arc::new(MemoryQueueStore::new());
    let adapter = build_adapter(
        &threads.client(),
        store.clone(),
        observer.clone(),
        agent_id(),
    )
    .await
    .expect("adapter");
    threads.answer_commands(|_| Err(Error::InvalidInput("refused".to_owned())));
    let thread = thread_ref();
    for _ in 0..3 {
        let turn = input_turn(vec![human_entry("x")]);
        br_agentic_contract::ThreadAccess::add_input_turn(&adapter, &thread, turn)
            .await
            .expect("accepted");
    }

    settled_over(&adapter, store.as_ref()).await;

    assert_eq!(threads.commands().len(), 3);
    assert!(observer.seen().is_empty());
    observer.open();
    eventually("three reports taken", || observer.seen().len() == 3).await;
    assert!(
        observer
            .seen()
            .iter()
            .all(|r| r.kind == ReportKind::ItemRefused)
    );
}
