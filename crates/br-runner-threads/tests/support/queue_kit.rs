use std::future::Future;
use std::time::Duration;

use br_agentic_contract::{PortError, ThreadRef};
use br_llm_messages::{EntryId, Uuid};
use br_runner_threads::test_double::{Command, CommandBody, MemoryThreads, Outcome};
use br_runner_threads::{
    Cause, Error, Failure, MessageStatus, QueueReport, QueueStore, ReportKind, ThreadAccessAdapter,
};

use super::ids::{human_entry, input_turn};
use super::world::World;

pub fn message_of(command: &Command) -> Uuid {
    match &command.body {
        CommandBody::Store(body) => body.message_id,
        CommandBody::Complete(body) => body.message_id,
        CommandBody::Fail(body) => body.message_id,
        CommandBody::Cancel(body) => body.message_id,
        CommandBody::CreateThread(_) => panic!("a thread creation names no message"),
    }
}

pub fn thread_of(command: &Command) -> Uuid {
    match &command.body {
        CommandBody::Store(body) => body.thread_id,
        CommandBody::Complete(body) => body.thread_id,
        CommandBody::Fail(body) => body.thread_id,
        CommandBody::Cancel(body) => body.thread_id,
        CommandBody::CreateThread(body) => body.thread_id,
    }
}

pub fn command_messages(threads: &MemoryThreads) -> Vec<Uuid> {
    threads.commands().iter().map(message_of).collect()
}

pub fn taken(command: &Command, status: MessageStatus) -> Outcome {
    Outcome {
        command_id: command.id,
        status: Some(status),
    }
}

pub fn unavailable() -> Error {
    Error::Unavailable(Failure::new(Cause::Transport, "the bus is down"))
}

pub fn lost() -> Error {
    Error::Lost(Failure::new(Cause::WaitFired, "no outcome"))
}

pub fn unreadable() -> Error {
    Error::Unreadable("the outcome cannot be read".to_owned())
}

pub async fn within<T>(what: &str, future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(2), future)
        .await
        .unwrap_or_else(|_| panic!("did not return in time: {what}"))
}

pub async fn pends<T>(future: impl Future<Output = T>) -> bool {
    tokio::time::timeout(Duration::from_millis(150), future)
        .await
        .is_err()
}

pub async fn quiet() {
    tokio::time::sleep(Duration::from_millis(150)).await;
}

pub fn fault_of(error: PortError) -> Error {
    match error {
        PortError::Fault(cause) => cause
            .downcast_ref::<Error>()
            .expect("the cause is an error of this library")
            .clone(),
        PortError::Refused(reason) => panic!("a refusal where a fault was expected: {reason:?}"),
    }
}

pub async fn write_input(world: &World, thread: &ThreadRef) -> (Uuid, EntryId) {
    let turn = input_turn(vec![human_entry("hello")]);
    let message = turn.id().as_uuid();
    let entry = turn.entries()[0].id();
    world
        .access()
        .add_input_turn(thread, turn)
        .await
        .expect("the input turn is accepted");
    (message, entry)
}

pub fn reports_of(world: &World, kind: ReportKind) -> Vec<QueueReport> {
    world
        .recorder
        .reports()
        .into_iter()
        .filter(|report| report.kind == kind)
        .collect()
}

pub async fn settled_over(adapter: &ThreadAccessAdapter, store: &(impl QueueStore + ?Sized)) {
    for _ in 0..400 {
        let kept = store.list().await.expect("the store lists");
        if adapter.queue_state().threads.is_empty() && kept.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the queue and the store never emptied");
}

pub async fn settled(world: &World) {
    settled_over(&world.adapter, world.store.as_ref()).await;
}
