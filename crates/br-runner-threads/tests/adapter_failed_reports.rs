mod support;

use std::sync::Arc;

use br_runner_threads::test_double::Outcome;
use br_runner_threads::{ItemKind, MemoryQueueStore, MessageStatus, ReportKind};
use support::adapter_x::*;
use support::*;

#[tokio::test]
async fn a_whole_message_that_threads_confirmed_as_failed_is_delivered_and_reported() {
    let rig = rig_with_store(Arc::new(MemoryQueueStore::new())).await;
    rig.threads.answer_commands(|command| {
        Ok(Outcome {
            command_id: command.id,
            status: Some(MessageStatus::Failed),
        })
    });
    let thread = thread_ref();
    let turn = input_turn(vec![notice_entry("hello")]);
    let message = turn.id().as_uuid();
    let entry = turn.entries()[0].id();

    rig.access()
        .add_input_turn(&thread, turn)
        .await
        .expect("accepted");

    rig.idle().await;
    let reports = rig.recorder.reports();
    assert!(
        reports
            .iter()
            .any(|report| report.kind == ReportKind::ClosingFailed
                && report.item_kind == ItemKind::WholeMessage
                && report.message_id == message),
        "{reports:?}"
    );
    assert_eq!(
        rig.adapter.entry_stored(&thread, entry).await,
        br_runner_threads::EntryOutcome::Stored
    );
}

#[tokio::test]
async fn an_opening_that_threads_confirmed_as_failed_is_reported_and_its_later_items_still_go() {
    let rig = rig_with_store(Arc::new(MemoryQueueStore::new())).await;
    rig.threads.answer_commands(|command| {
        Ok(Outcome {
            command_id: command.id,
            status: Some(MessageStatus::Failed),
        })
    });
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);

    rig.access()
        .add_entry(&thread, turn.clone(), notice("work"))
        .await
        .expect("accepted");

    rig.idle().await;
    assert!(
        rig.recorder
            .reports()
            .iter()
            .any(|report| report.kind == ReportKind::ClosingFailed
                && report.item_kind == ItemKind::Opening
                && report.message_id == turn.id.as_uuid())
    );
    assert_eq!(rig.threads.chunks().len(), 1);
}
