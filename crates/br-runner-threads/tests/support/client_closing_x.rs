use br_llm_messages::ClosureReason;
use br_runner_threads::test_double;
use br_runner_threads::test_double::{CommandBody, MemoryThreads};
use br_runner_threads::{Cause, ClosureKind, Error, Failure, OpenStream, StreamHandle};
use contract_threads::{StreamSeal, seal_hash};

use super::client_x::*;
use super::ids::*;

pub const NON_COMPLETED: [ClosureKind; 4] = [
    ClosureKind::BudgetExhausted,
    ClosureKind::Failed,
    ClosureKind::Interrupted,
    ClosureKind::Cancelled,
];

pub async fn with_chunks(threads: &MemoryThreads, count: usize) -> (StreamHandle, OpenStream) {
    let (handle, open) = opened(&threads.client()).await;
    for index in 0..count {
        if index % 2 == 0 {
            handle.entry(&notice("filler")).await.expect("taken");
        } else {
            handle.keepalive().await.expect("taken");
        }
    }
    (handle, open)
}

pub fn seal_of(threads: &MemoryThreads) -> StreamSeal {
    let chunks = chunk_list(threads);
    StreamSeal {
        last_seq: chunks.len() as u64 - 1,
        hash: seal_hash(chunks.iter()).expect("hash"),
    }
}

pub fn seal_in(body: &CommandBody) -> Option<StreamSeal> {
    match body {
        CommandBody::Complete(complete) => Some(complete.seal.clone()),
        CommandBody::Fail(fail) => fail.seal.clone(),
        other => panic!("not a closing: {other:?}"),
    }
}

pub fn closure_in(body: &CommandBody) -> ClosureReason {
    let closure = match body {
        CommandBody::Complete(complete) => complete.closure.as_ref(),
        CommandBody::Fail(fail) => fail.closure.as_ref(),
        other => panic!("not a closing: {other:?}"),
    };
    closure
        .expect("the closing names its closure")
        .to_closure_reason()
        .expect("the closure reads back through the library")
}

pub fn unavailable() -> Error {
    Error::Unavailable(test_double::failure(Cause::NoListener, "none"))
}

pub fn lost() -> Error {
    Error::Lost(test_double::failure(Cause::WaitFired, "no outcome"))
}

pub fn kind_of(reason: &ClosureReason) -> ClosureKind {
    match reason {
        ClosureReason::Completed { .. } => ClosureKind::Completed,
        ClosureReason::Cancelled { .. } => ClosureKind::Cancelled,
        ClosureReason::Interrupted { .. } => ClosureKind::Interrupted,
        ClosureReason::BudgetExhausted { .. } => ClosureKind::BudgetExhausted,
        ClosureReason::Failed { .. } => ClosureKind::Failed,
        other => panic!("a reason this test does not know: {other:?}"),
    }
}
