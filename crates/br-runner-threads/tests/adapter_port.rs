use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use br_agentic_contract::{Fields, PortError, Reason, ThreadPort};
use br_runner_threads::{Cause, Error, Failure, ThreadPortAdapter};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Snapshot {
    thread: Option<Uuid>,
}

impl Fields for Snapshot {
    type Update = ();

    fn empty_update() -> Self::Update {}

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, _update: Self::Update) -> Result<(), Reason> {
        Ok(())
    }
}

fn finding_the_snapshot_thread() -> ThreadPortAdapter<Snapshot> {
    ThreadPortAdapter::new(|snapshot: &Snapshot| {
        let found = snapshot.thread;
        Box::pin(async move { Ok(found) })
    })
}

#[tokio::test]
async fn the_thread_reference_wraps_the_id_that_the_function_found() {
    let port = finding_the_snapshot_thread();
    let id = Uuid::now_v7();

    let thread = port
        .thread(&Snapshot { thread: Some(id) })
        .await
        .expect("a thread");

    assert_eq!(thread.as_uuid(), id);
}

#[tokio::test]
async fn two_snapshots_with_two_ids_give_two_thread_references() {
    let port = finding_the_snapshot_thread();

    let first = port
        .thread(&Snapshot {
            thread: Some(Uuid::now_v7()),
        })
        .await
        .expect("a thread");
    let second = port
        .thread(&Snapshot {
            thread: Some(Uuid::now_v7()),
        })
        .await
        .expect("a thread");

    assert_ne!(first, second);
}

#[tokio::test]
async fn a_snapshot_with_no_thread_id_is_a_fault_with_invalid_input_as_cause() {
    let port = finding_the_snapshot_thread();

    let result = port.thread(&Snapshot { thread: None }).await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
}

#[tokio::test]
async fn an_id_that_is_not_a_uuid_v7_is_a_fault_with_invalid_input_as_cause() {
    let port = finding_the_snapshot_thread();

    let result = port
        .thread(&Snapshot {
            thread: Some(Uuid::from_u128(0x1234_5678_9abc_4def_8123_4567_89ab_cdef)),
        })
        .await;

    assert!(matches!(fault_of(result), Error::InvalidInput(_)));
}

#[tokio::test]
async fn a_failure_of_the_function_reaches_the_node_unchanged() {
    let port: ThreadPortAdapter<Snapshot> = ThreadPortAdapter::new(|_snapshot| {
        Box::pin(async {
            Err(PortError::fault(Error::Unavailable(Failure::new(
                Cause::Transport,
                "the lookup is down",
            ))))
        })
    });

    let result = port.thread(&Snapshot { thread: None }).await;

    assert_eq!(
        fault_of(result),
        Error::Unavailable(Failure::new(Cause::Transport, "the lookup is down"))
    );
}

#[tokio::test]
async fn a_refusal_of_the_function_stays_a_refusal() {
    let port: ThreadPortAdapter<Snapshot> = ThreadPortAdapter::new(|_snapshot| {
        Box::pin(async { Err(PortError::refused("no thread")) })
    });

    let result = port.thread(&Snapshot { thread: None }).await;

    assert!(result.expect_err("an error").is_refusal());
}

#[tokio::test]
async fn the_function_is_asked_again_for_every_call() {
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&asked);
    let port: ThreadPortAdapter<Snapshot> = ThreadPortAdapter::new(move |snapshot: &Snapshot| {
        counter.fetch_add(1, Ordering::SeqCst);
        let found = snapshot.thread;
        Box::pin(async move { Ok(found) })
    });
    let snapshot = Snapshot {
        thread: Some(Uuid::now_v7()),
    };

    port.thread(&snapshot).await.expect("a thread");
    port.thread(&snapshot).await.expect("a thread");

    assert_eq!(asked.load(Ordering::SeqCst), 2);
}

fn fault_of<T>(result: Result<T, PortError>) -> Error {
    match result {
        Err(PortError::Fault(cause)) => cause
            .downcast_ref::<Error>()
            .expect("the cause of a fault is the error of this library")
            .clone(),
        Err(PortError::Refused(reason)) => panic!("a refusal, not a fault: {reason}"),
        Ok(_) => panic!("the thread was found"),
    }
}
