mod support;

use std::sync::Arc;

use br_agentic_contract::AiTurnRef;
use br_llm_messages::{ClosureReason, TurnClosure};
use br_runner_threads::test_double::{CommandBody, Operation};
use br_runner_threads::{ClosureKind, Error, MemoryQueueStore, ReportKind, WriteContext};
use contract_threads::{Chunk, StreamSeal, seal_hash};
use support::adapter_wire::*;
use support::adapter_x::*;
use support::client_closing_x::{closure_in, kind_of};
use support::*;

fn closing(reason: ClosureReason) -> TurnClosure {
    TurnClosure::new(entry_id(), reason)
}

async fn plain_rig() -> Rig {
    rig_with_store(Arc::new(MemoryQueueStore::new())).await
}

async fn turn_with_entries(rig: &Rig, count: usize) -> (br_agentic_contract::ThreadRef, AiTurnRef) {
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

async fn last_command(rig: &Rig) -> CommandBody {
    eventually("the thread queue is empty", || {
        rig.adapter.queue_state().threads.is_empty()
    })
    .await;
    rig.threads
        .commands()
        .last()
        .expect("a command")
        .body
        .clone()
}

fn seal_of_sent(rig: &Rig, turn: &AiTurnRef) -> StreamSeal {
    let chunks: Vec<Chunk> = chunks_of(&rig.threads, turn.id.as_uuid())
        .into_iter()
        .map(|(_, chunk)| chunk)
        .collect();
    StreamSeal {
        last_seq: chunks.len() as u64 - 1,
        hash: seal_hash(chunks.iter()).expect("hash"),
    }
}

async fn close_healthy(reason: ClosureReason) -> (CommandBody, StreamSeal) {
    let rig = plain_rig().await;
    let (thread, turn) = turn_with_entries(&rig, 3).await;
    rig.access()
        .close_turn(&thread, turn.clone(), closing(reason))
        .await
        .expect("accepted");
    let body = last_command(&rig).await;
    (body, seal_of_sent(&rig, &turn))
}

async fn close_unknown(reason: ClosureReason) -> CommandBody {
    let rig = plain_rig().await;
    rig.access()
        .close_turn(&thread_ref(), turn_ref(rig.agent), closing(reason))
        .await
        .expect("accepted");
    last_command(&rig).await
}

#[tokio::test]
async fn completed_goes_as_a_completion_sealed_over_every_chunk_of_the_turn() {
    let (body, seal) = close_healthy(ClosureReason::completed()).await;

    let CommandBody::Complete(complete) = body else {
        panic!("a completion");
    };
    assert_eq!(complete.seal, seal);
    assert_eq!(complete.seal.last_seq, 2);
}

#[tokio::test]
async fn every_reason_but_completed_goes_as_a_failure_that_names_its_closure_and_its_detail() {
    for reason in [
        ClosureReason::failed("it broke").expect("reason"),
        ClosureReason::interrupted()
            .with_detail("the host stopped")
            .expect("reason"),
        ClosureReason::budget_exhausted()
            .with_detail("no step left")
            .expect("reason"),
        ClosureReason::cancelled(),
    ] {
        let (body, seal) = close_healthy(reason.clone()).await;

        let CommandBody::Fail(fail) = &body else {
            panic!("{reason:?} is not a failure");
        };
        assert_eq!(fail.seal, Some(seal));
        let sent = closure_in(&body);
        assert_eq!(kind_of(&sent), kind_of(&reason));
        assert_eq!(sent.detail(), reason.detail());
    }
}

#[tokio::test]
async fn completed_names_its_closure() {
    let (body, _) = close_healthy(ClosureReason::completed()).await;

    assert_eq!(kind_of(&closure_in(&body)), ClosureKind::Completed);
}

#[tokio::test]
async fn budget_exhausted_and_cancelled_are_not_reported_as_failed_closings() {
    for reason in [
        ClosureReason::budget_exhausted(),
        ClosureReason::cancelled(),
    ] {
        let rig = plain_rig().await;
        let (thread, turn) = turn_with_entries(&rig, 1).await;
        rig.access()
            .close_turn(&thread, turn, closing(reason))
            .await
            .expect("accepted");
        rig.idle().await;
        assert!(
            rig.recorder
                .reports()
                .iter()
                .all(|report| report.kind != ReportKind::ClosingFailed)
        );
    }
}

#[tokio::test]
async fn a_closing_names_the_turn_and_the_thread_and_takes_the_acting_user_of_the_write_context() {
    let user = v7();
    let agent = agent_id();
    let rig = rig_with_context(WriteContext {
        acting_user_id: Some(user),
        ..context_of(agent)
    })
    .await;
    let (thread, turn) = turn_with_entries(&rig, 1).await;

    rig.access()
        .close_turn(&thread, turn.clone(), closing(ClosureReason::completed()))
        .await
        .expect("accepted");

    let CommandBody::Complete(complete) = last_command(&rig).await else {
        panic!("a completion");
    };
    assert_eq!(complete.message_id, turn.id.as_uuid());
    assert_eq!(complete.thread_id, thread.as_uuid());
    assert_eq!(complete.acting_user_id, Some(user));
}

#[tokio::test]
async fn a_closing_goes_to_threads_after_every_chunk_accepted_before_it() {
    let rig = plain_rig().await;
    let (thread, turn) = turn_with_entries(&rig, 2).await;

    rig.access()
        .close_turn(&thread, turn, closing(ClosureReason::completed()))
        .await
        .expect("accepted");

    rig.idle().await;
    let operations = rig.threads.operations();
    assert_eq!(operations.len(), 4);
    assert!(matches!(operations[0], Operation::Command(_)));
    assert!(matches!(operations[1], Operation::Chunk { seq: 0, .. }));
    assert!(matches!(operations[2], Operation::Chunk { seq: 1, .. }));
    let Operation::Command(last) = &operations[3] else {
        panic!("the closing comes last");
    };
    assert!(matches!(last.body, CommandBody::Complete(_)));
}

#[tokio::test]
async fn a_turn_the_adapter_never_saw_is_closed_by_its_id_with_no_seal() {
    for reason in [
        ClosureReason::failed("it broke").expect("reason"),
        ClosureReason::interrupted(),
        ClosureReason::budget_exhausted(),
        ClosureReason::cancelled(),
    ] {
        let CommandBody::Fail(fail) = close_unknown(reason).await else {
            panic!("a failure");
        };
        assert_eq!(fail.seal, None);
    }
}

#[tokio::test]
async fn a_turn_with_state_and_no_chunk_is_closed_by_its_id_with_no_seal() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    rig.access()
        .open_result(&thread, turn.clone(), entry_id())
        .await
        .expect("accepted");

    rig.access()
        .close_turn(
            &thread,
            turn,
            closing(ClosureReason::failed("it broke").expect("reason")),
        )
        .await
        .expect("accepted");

    let CommandBody::Fail(fail) = last_command(&rig).await else {
        panic!("a failure");
    };
    assert_eq!(fail.seal, None);
}

#[tokio::test]
async fn completed_is_refused_for_a_turn_with_no_state() {
    let rig = plain_rig().await;

    let result = rig
        .access()
        .close_turn(
            &thread_ref(),
            turn_ref(rig.agent),
            closing(ClosureReason::completed()),
        )
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    assert!(rig.adapter.queue_state().threads.is_empty());
    assert!(rig.threads.operations().is_empty());
}

#[tokio::test]
async fn completed_is_refused_for_a_turn_with_state_and_no_chunk() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    rig.access()
        .open_result(&thread, turn.clone(), entry_id())
        .await
        .expect("accepted");
    rig.idle().await;

    let result = rig
        .access()
        .close_turn(&thread, turn, closing(ClosureReason::completed()))
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
    assert_eq!(rig.threads.commands().len(), 1);
}
