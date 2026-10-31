mod support;

use std::time::Duration;

use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{Cause, Error, Failure};
use support::client_x::*;
use support::*;

fn lost() -> Error {
    Error::Lost(Failure::new(Cause::WaitFired, "the bus did not answer"))
}

async fn quiet() {
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn a_chunk_the_bus_does_not_take_breaks_the_stream_whatever_the_cause() {
    let causes = [
        lost(),
        Error::Unavailable(Failure::new(Cause::NoListener, "none")),
        Error::Unauthenticated,
        Error::Unreadable("garbled".to_owned()),
    ];
    for cause in causes {
        let threads = MemoryThreads::new();
        let (handle, _) = opened(&threads.client()).await;
        threads.answer_chunks(move |_, _, _| Err(cause.clone()));

        let result = handle.keepalive().await;

        assert_eq!(result, Err(Error::StreamBroken));
    }
}

#[tokio::test]
async fn after_a_broken_chunk_every_later_chunk_is_refused_without_reaching_the_bus() {
    let threads = MemoryThreads::new();
    let (handle, open) = opened(&threads.client()).await;
    let writer = handle.writer();
    threads.answer_chunks(|_, _, _| Err(lost()));
    handle.keepalive().await.expect_err("broken");
    let sent = threads.chunks().len();

    let by_handle = handle.entry(&notice("a")).await;
    let by_writer = writer.keepalive().await;
    let delta = handle.delta(&started(open.message_id, entry_id())).await;

    assert_eq!(by_handle, Err(Error::StreamBroken));
    assert_eq!(by_writer, Err(Error::StreamBroken));
    assert_eq!(delta, Err(Error::StreamBroken));
    assert_eq!(threads.chunks().len(), sent);
}

#[tokio::test]
async fn a_broken_stream_is_reported_before_the_content_of_a_chunk_is_judged() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    threads.answer_chunks(|_, _, _| Err(lost()));
    handle.keepalive().await.expect_err("broken");

    let foreign = handle.delta(&started(turn_id(), entry_id())).await;

    assert_eq!(foreign, Err(Error::StreamBroken));
}

#[tokio::test]
async fn a_chunk_operation_returns_only_when_the_bus_has_taken_the_chunk() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    threads.pause();
    let writer = handle.writer();
    let task = tokio::spawn(async move { writer.keepalive().await });
    eventually("the chunk reached the bus", || threads.chunks().len() == 1).await;

    quiet().await;
    let finished_while_paused = task.is_finished();
    threads.resume();
    let result = task.await.expect("joined");

    assert!(!finished_while_paused);
    assert_eq!(result, Ok(()));
}

#[tokio::test]
async fn abandoning_a_chunk_operation_after_the_chunk_was_sent_breaks_the_handle() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    threads.pause();
    let writer = handle.writer();
    let task = tokio::spawn(async move { writer.keepalive().await });
    eventually("the chunk reached the bus", || threads.chunks().len() == 1).await;

    task.abort();
    let _ = task.await;
    threads.resume();
    let after = handle.entry(&notice("later")).await;

    assert_eq!(after, Err(Error::StreamBroken));
    assert_eq!(threads.chunks().len(), 1);
}

#[tokio::test]
async fn a_writer_sees_the_break_that_the_handle_met_and_the_other_way_round() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    let writer = handle.writer();
    threads.answer_chunks(|_, _, _| Err(lost()));
    writer.keepalive().await.expect_err("broken");
    threads.answer_chunks(|_, _, _| Ok(()));

    let by_handle = handle.keepalive().await;

    assert_eq!(by_handle, Err(Error::StreamBroken));
}

#[tokio::test]
async fn dropping_the_handle_a_writer_a_sink_and_a_control_sends_nothing() {
    let threads = MemoryThreads::new();
    let (handle, _) = opened(&threads.client()).await;
    let writer = handle.writer();
    let (sink, control) = handle.response(entry_id()).expect("a pair");
    let (writer_sink, writer_control) = writer.response(entry_id()).expect("a pair");
    let operations = threads.operations().len();

    drop((sink, control, writer_sink, writer_control, writer, handle));
    quiet().await;

    assert_eq!(threads.operations().len(), operations);
}
