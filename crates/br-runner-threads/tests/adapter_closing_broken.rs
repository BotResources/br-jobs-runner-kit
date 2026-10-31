mod support;

use std::sync::Arc;

use br_llm_messages::{ClosureReason, TurnClosure};
use br_runner_threads::test_double::CommandBody;
use br_runner_threads::{Error, MemoryQueueStore};
use support::adapter_x::*;
use support::*;

async fn broken_turn_closed_with(reason: ClosureReason) -> (Rig, CommandBody) {
    let rig = rig_with_store(Arc::new(MemoryQueueStore::new())).await;
    rig.threads.answer_chunks(|_, seq, _| {
        if seq == 0 {
            Err(Error::InvalidInput("refused".to_owned()))
        } else {
            Ok(())
        }
    });
    let (thread, turn) = (thread_ref(), turn_ref(rig.agent));
    for text in ["refused", "taken"] {
        rig.access()
            .add_entry(&thread, turn.clone(), notice(text))
            .await
            .expect("accepted");
    }
    rig.idle().await;
    rig.access()
        .close_turn(&thread, turn, TurnClosure::new(entry_id(), reason))
        .await
        .expect("a broken turn does not refuse the closing");
    rig.idle().await;
    let body = rig
        .threads
        .commands()
        .last()
        .expect("a command")
        .body
        .clone();
    (rig, body)
}

#[tokio::test]
async fn a_failure_closing_of_a_turn_with_a_chunk_that_ended_refused_carries_no_seal() {
    for reason in [
        ClosureReason::failed("it broke").expect("reason"),
        ClosureReason::interrupted(),
        ClosureReason::budget_exhausted(),
        ClosureReason::cancelled(),
    ] {
        let (_rig, body) = broken_turn_closed_with(reason.clone()).await;

        let CommandBody::Fail(fail) = body else {
            panic!("{reason:?} is not a failure");
        };
        assert_eq!(fail.seal, None, "{reason:?}");
    }
}

#[tokio::test]
async fn a_completed_closing_of_a_turn_with_a_chunk_that_ended_refused_still_goes_with_its_seal() {
    let (_rig, body) = broken_turn_closed_with(ClosureReason::completed()).await;

    let CommandBody::Complete(complete) = body else {
        panic!("a completion");
    };
    assert_eq!(complete.seal.last_seq, 1);
}
