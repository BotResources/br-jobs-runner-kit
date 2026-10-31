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
            handle.keepalive().await.expect("taken");
        } else {
            handle.entry(&notice("filler")).await.expect("taken");
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
        CommandBody::Cancel(cancel) => cancel.seal.clone(),
        other => panic!("not a closing: {other:?}"),
    }
}

pub fn unavailable() -> Error {
    Error::Unavailable(test_double::failure(Cause::NoListener, "none"))
}

pub fn lost() -> Error {
    Error::Lost(test_double::failure(Cause::WaitFired, "no outcome"))
}
