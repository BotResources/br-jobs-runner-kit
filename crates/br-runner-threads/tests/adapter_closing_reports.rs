mod support;

use std::sync::Arc;

use br_llm_messages::{ClosureReason, TurnClosure};
use br_runner_threads::test_double::{CommandBody, Outcome};
use br_runner_threads::{Error, MemoryQueueStore, MessageStatus, ReportKind};
use support::adapter_x::*;
use support::*;

fn closing(reason: ClosureReason) -> TurnClosure {
    TurnClosure::new(entry_id(), reason)
}

async fn plain_rig() -> Rig {
    rig_with_store(Arc::new(MemoryQueueStore::new())).await
}

async fn turn_with_entries(
    rig: &Rig,
    count: usize,
) -> (
    br_agentic_contract::ThreadRef,
    br_agentic_contract::AiTurnRef,
) {
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    for n in 0..count {
        rig.access()
            .add_entry(&thread, turn.clone(), notice(&format!("entry {n}")))
            .await
            .expect("accepted");
    }
    (thread, turn)
}

#[tokio::test]
async fn a_closing_that_threads_confirmed_as_failed_is_reported() {
    let rig = plain_rig().await;
    rig.threads.answer_commands(|command| {
        let status = match command.body {
            CommandBody::Store(_) => MessageStatus::Streaming,
            _ => MessageStatus::Failed,
        };
        Ok(Outcome {
            command_id: command.id,
            status: Some(status),
        })
    });
    let (thread, turn) = turn_with_entries(&rig, 1).await;

    rig.access()
        .close_turn(&thread, turn.clone(), closing(ClosureReason::completed()))
        .await
        .expect("accepted");

    eventually("the report", || {
        rig.recorder
            .reports()
            .iter()
            .any(|r| r.kind == ReportKind::ClosingFailed && r.message_id == turn.id.as_uuid())
    })
    .await;
    rig.idle().await;
}

#[tokio::test]
async fn a_closing_that_threads_refused_for_good_is_reported_with_its_error() {
    let rig = plain_rig().await;
    rig.threads.answer_commands(|command| match command.body {
        CommandBody::Store(_) => Ok(Outcome {
            command_id: command.id,
            status: Some(MessageStatus::Streaming),
        }),
        _ => Err(Error::Unauthenticated),
    });
    let (thread, turn) = turn_with_entries(&rig, 1).await;

    rig.access()
        .close_turn(&thread, turn, closing(ClosureReason::completed()))
        .await
        .expect("accepted");

    eventually("the report", || {
        rig.recorder
            .reports()
            .iter()
            .any(|r| r.kind == ReportKind::ClosingFailed && r.error == Some(Error::Unauthenticated))
    })
    .await;
}
