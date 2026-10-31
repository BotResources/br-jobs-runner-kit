mod support;

#[path = "../examples/close_left_open_turn.rs"]
#[allow(dead_code)] // the example is included for its functions; its main is unused here
mod close_left_open_turn;
#[path = "../examples/queue_not_draining.rs"]
#[allow(dead_code)] // the example is included for its functions; its main is unused here
mod queue_not_draining;

use br_runner_threads::test_double;
use std::sync::Arc;
use std::time::Duration;

use br_runner_threads::test_double::{CommandBody, MemoryThreads};
use br_runner_threads::{Error, ItemKind, MessageStatus, ReportKind};
use close_left_open_turn::{close_by_message_id, leave_a_turn_open};
use queue_not_draining::{
    CollectedReports, StuckThread, accept_an_input_turn, adapter_observed_by, stuck_threads,
};
use support::adapter_wire::{ai_of, is_streaming, stores};
use support::queue_kit::{message_of, thread_of, unavailable, within};
use support::*;

#[tokio::test]
async fn a_thread_the_bus_does_not_take_is_listed_as_stuck_with_its_oldest_message_and_last_error()
{
    let threads = MemoryThreads::new();
    threads.answer_commands(|_| Err(unavailable()));
    let observer = Arc::new(CollectedReports::default());
    let adapter = adapter_observed_by(&threads.client(), observer)
        .await
        .expect("adapter");
    let thread = thread_ref();

    let message_id = accept_an_input_turn(&adapter, &thread).await;

    eventually("a failed attempt is shown", || {
        adapter
            .queue_state()
            .threads
            .iter()
            .any(|state| state.last_failure.is_some())
    })
    .await;
    assert_eq!(
        stuck_threads(&adapter, Duration::ZERO),
        vec![StuckThread {
            thread,
            waiting: 1,
            message_id,
            item_kind: ItemKind::WholeMessage,
            last_error: Some(unavailable()),
        }]
    );
}

#[tokio::test]
async fn a_thread_that_waits_less_than_the_threshold_is_not_listed() {
    let threads = MemoryThreads::new();
    threads.pause();
    let observer = Arc::new(CollectedReports::default());
    let adapter = adapter_observed_by(&threads.client(), observer)
        .await
        .expect("adapter");
    accept_an_input_turn(&adapter, &thread_ref()).await;

    let stuck = stuck_threads(&adapter, Duration::from_secs(3600));

    assert!(stuck.is_empty());
    assert_eq!(stuck_threads(&adapter, Duration::ZERO).len(), 1);
}

#[tokio::test]
async fn a_bus_that_takes_the_items_leaves_no_stuck_thread() {
    let threads = MemoryThreads::new();
    let observer = Arc::new(CollectedReports::default());
    let adapter = adapter_observed_by(&threads.client(), observer)
        .await
        .expect("adapter");
    accept_an_input_turn(&adapter, &thread_ref()).await;

    eventually("the queue is empty", || {
        adapter.queue_state().threads.is_empty()
    })
    .await;

    assert!(stuck_threads(&adapter, Duration::ZERO).is_empty());
}

#[tokio::test]
async fn the_observer_collects_each_failed_attempt_with_the_message_the_kind_and_the_error() {
    let threads = MemoryThreads::new();
    threads.answer_commands(|_| Err(unavailable()));
    let observer = Arc::new(CollectedReports::default());
    let adapter = adapter_observed_by(&threads.client(), observer.clone())
        .await
        .expect("adapter");

    let message_id = accept_an_input_turn(&adapter, &thread_ref()).await;

    eventually("an attempt failed", || {
        !observer.of_kind(ReportKind::AttemptFailed).is_empty()
    })
    .await;
    let report = observer.of_kind(ReportKind::AttemptFailed).remove(0);
    assert_eq!(report.message_id, message_id);
    assert_eq!(report.item_kind, ItemKind::WholeMessage);
    assert_eq!(report.error, Some(unavailable()));
}

#[tokio::test]
async fn dropping_the_adapter_reports_the_waiting_item_as_unsent() {
    let threads = MemoryThreads::new();
    threads.answer_commands(|_| Err(unavailable()));
    let observer = Arc::new(CollectedReports::default());
    let adapter = adapter_observed_by(&threads.client(), observer.clone())
        .await
        .expect("adapter");
    let message_id = accept_an_input_turn(&adapter, &thread_ref()).await;

    drop(adapter);

    eventually("the item is reported unsent", || {
        !observer.of_kind(ReportKind::Unsent).is_empty()
    })
    .await;
    let unsent = observer.of_kind(ReportKind::Unsent);
    assert_eq!(unsent.len(), 1);
    assert_eq!(unsent[0].message_id, message_id);
}

#[tokio::test]
async fn a_turn_left_open_by_an_ended_instance_stays_streaming() {
    let world = world().await;
    let turn = turn_ref(world.agent);

    within(
        "the turn is left open",
        leave_a_turn_open(&world.adapter, &thread_ref(), &turn),
    )
    .await
    .expect("accepted");

    let stored = stores(&world.threads);
    assert_eq!(stored.len(), 1);
    assert!(is_streaming(&stored[0]));
    assert_eq!(ai_of(&stored[0]).agent_id, world.agent.as_uuid());
    assert!(!world.threads.commands().iter().any(|command| matches!(
        command.body,
        CommandBody::Complete(_) | CommandBody::Fail(_)
    )));
}

#[tokio::test]
async fn closing_by_the_message_id_fails_the_left_open_turn_with_no_seal() {
    let world = world().await;
    let (thread, turn) = (thread_ref(), turn_ref(world.agent));
    leave_a_turn_open(&world.adapter, &thread, &turn)
        .await
        .expect("accepted");

    let confirmed = close_by_message_id(&world.client, &thread, turn.id.as_uuid())
        .await
        .expect("confirmed");

    assert_eq!(confirmed.status, Some(MessageStatus::Failed));
    let closing = world.threads.commands().pop().expect("a command");
    assert_eq!(message_of(&closing), turn.id.as_uuid());
    assert_eq!(thread_of(&closing), thread.as_uuid());
    match closing.body {
        CommandBody::Fail(fail) => assert!(fail.seal.is_none()),
        other => panic!("not a failure: {other:?}"),
    }
}

#[tokio::test]
async fn a_message_this_process_never_opened_is_closed_by_its_id_too() {
    let world = world().await;
    let (thread, message) = (thread_ref(), v7());

    close_by_message_id(&world.client, &thread, message)
        .await
        .expect("confirmed");

    let closing = world.threads.commands().pop().expect("a command");
    assert!(matches!(closing.body, CommandBody::Fail(_)));
    assert_eq!(message_of(&closing), message);
}

#[tokio::test]
async fn a_closing_that_threads_refuses_gives_the_refusal_to_the_runner() {
    let world = world().await;
    world.threads.answer_commands(|command| {
        Err(Error::Refused(test_double::refusal(
            "MESSAGE_NOT_STREAMING",
            "the message is closed",
            Some(command.id),
        )))
    });

    let result = close_by_message_id(&world.client, &thread_ref(), v7()).await;

    assert!(
        matches!(result, Err(Error::Refused(refusal)) if refusal.code == "MESSAGE_NOT_STREAMING")
    );
}
