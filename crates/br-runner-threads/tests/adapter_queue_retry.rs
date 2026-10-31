mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use br_runner_threads::{Error, ItemKind, MessageStatus, ReportKind};
use support::queue_kit::*;
use support::*;

async fn command_is_sent_again_until_threads_takes_it(error: Error) {
    let world = world().await;
    let thread = thread_ref();
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&attempts);
    let failure = error.clone();
    world.threads.answer_commands(move |command| {
        if counter.fetch_add(1, Ordering::SeqCst) < 2 {
            Err(failure.clone())
        } else {
            Ok(taken(command, MessageStatus::Completed))
        }
    });
    let (message, _) = write_input(&world, &thread).await;

    settled(&world).await;

    let commands = world.threads.commands();
    assert_eq!(commands.len(), 3);
    assert!(commands.iter().all(|sent| sent == &commands[0]));
    eventually("two failed attempts reported", || {
        reports_of(&world, ReportKind::AttemptFailed).len() == 2
    })
    .await;
    for report in reports_of(&world, ReportKind::AttemptFailed) {
        assert_eq!(report.error, Some(error.clone()));
        assert_eq!(report.item_kind, ItemKind::WholeMessage);
        assert_eq!(report.message_id, message);
        assert_eq!(report.item_id, commands[0].id);
        assert_eq!(report.thread, thread);
    }
    assert!(reports_of(&world, ReportKind::ItemRefused).is_empty());
}

#[tokio::test]
async fn a_command_that_threads_could_not_be_reached_for_is_sent_again() {
    command_is_sent_again_until_threads_takes_it(unavailable()).await;
}

#[tokio::test]
async fn a_command_whose_outcome_was_lost_is_sent_again() {
    command_is_sent_again_until_threads_takes_it(lost()).await;
}

#[tokio::test]
async fn a_command_whose_outcome_cannot_be_read_is_sent_again() {
    command_is_sent_again_until_threads_takes_it(unreadable()).await;
}

async fn command_failure_is_final_reported_and_removed(error: Error) {
    let world = world().await;
    let thread = thread_ref();
    let turn = input_turn(vec![human_entry("refused")]);
    let first = turn.id().as_uuid();
    let failure = error.clone();
    world.threads.answer_commands(move |command| {
        if message_of(command) == first {
            Err(failure.clone())
        } else {
            Ok(taken(command, MessageStatus::Completed))
        }
    });
    world
        .access()
        .add_input_turn(&thread, turn)
        .await
        .expect("accepted");
    let (second, _) = write_input(&world, &thread).await;

    settled(&world).await;

    assert_eq!(command_messages(&world.threads), vec![first, second]);
    eventually("the refusal is reported", || {
        !reports_of(&world, ReportKind::ItemRefused).is_empty()
    })
    .await;
    let refused = reports_of(&world, ReportKind::ItemRefused);
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].error, Some(error));
    assert_eq!(refused[0].message_id, first);
    assert_eq!(refused[0].item_kind, ItemKind::WholeMessage);
    assert_eq!(refused[0].thread, thread);
    assert!(reports_of(&world, ReportKind::AttemptFailed).is_empty());
}

#[tokio::test]
async fn an_invalid_input_failure_of_a_command_is_final() {
    command_failure_is_final_reported_and_removed(Error::InvalidInput("refused".to_owned())).await;
}

#[tokio::test]
async fn an_unauthenticated_failure_of_a_command_is_final() {
    command_failure_is_final_reported_and_removed(Error::Unauthenticated).await;
}

#[tokio::test]
async fn an_infra_missing_failure_of_a_command_is_final() {
    command_failure_is_final_reported_and_removed(Error::InfraMissing("the stream".to_owned()))
        .await;
}

#[tokio::test]
async fn a_stream_broken_failure_of_a_command_is_final() {
    command_failure_is_final_reported_and_removed(Error::StreamBroken).await;
}

#[tokio::test]
async fn a_stream_closed_failure_of_a_command_is_final() {
    command_failure_is_final_reported_and_removed(Error::StreamClosed).await;
}

#[tokio::test]
async fn a_store_failure_of_a_command_is_final() {
    command_failure_is_final_reported_and_removed(Error::Store("kept".to_owned())).await;
}

#[tokio::test]
async fn a_chunk_that_the_bus_did_not_take_is_sent_again_identical() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&attempts);
    world.threads.answer_chunks(move |_, _, _| {
        if counter.fetch_add(1, Ordering::SeqCst) < 2 {
            Err(Error::StreamBroken)
        } else {
            Ok(())
        }
    });
    world
        .access()
        .add_entry(&thread, turn.clone(), notice("n"))
        .await
        .expect("accepted");

    settled(&world).await;

    let chunks = world.threads.chunks();
    assert_eq!(chunks.len(), 3);
    assert!(chunks.iter().all(|sent| sent == &chunks[0]));
    assert_eq!(chunks[0].1, 0);
    assert_eq!(world.threads.commands().len(), 1);
    eventually("two failed attempts reported", || {
        reports_of(&world, ReportKind::AttemptFailed).len() == 2
    })
    .await;
    for report in reports_of(&world, ReportKind::AttemptFailed) {
        assert_eq!(report.error, Some(Error::StreamBroken));
        assert_eq!(report.item_kind, ItemKind::Chunk);
        assert_eq!(report.message_id, turn.id.as_uuid());
    }
}

#[tokio::test]
async fn a_chunk_failure_other_than_stream_broken_is_final_and_later_chunks_still_go() {
    let world = world().await;
    let thread = thread_ref();
    let turn = turn_ref(world.agent);
    world.threads.answer_chunks(|_, seq, _| {
        if seq == 0 {
            Err(Error::InvalidInput("refused".to_owned()))
        } else {
            Ok(())
        }
    });
    for text in ["first", "second"] {
        world
            .access()
            .add_entry(&thread, turn.clone(), notice(text))
            .await
            .expect("accepted");
    }

    settled(&world).await;

    let seqs: Vec<u64> = world.threads.chunks().iter().map(|c| c.1).collect();
    assert_eq!(seqs, vec![0, 1]);
    eventually("the refusal is reported", || {
        !reports_of(&world, ReportKind::ItemRefused).is_empty()
    })
    .await;
    let refused = reports_of(&world, ReportKind::ItemRefused);
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].item_kind, ItemKind::Chunk);
    assert_eq!(
        refused[0].error,
        Some(Error::InvalidInput("refused".to_owned()))
    );
}
