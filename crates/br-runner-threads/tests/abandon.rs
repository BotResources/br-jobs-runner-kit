mod support;

use std::sync::Arc;
use std::time::Duration;

use br_runner_threads::ClosureKind;
use br_runner_threads::transport::ChunkMode;
use contract_threads::ChunkFrame;
use support::*;
use tokio::sync::Notify;

async fn settle() {
    tokio::time::sleep(Duration::from_millis(50)).await;
}

fn seqs(world: &World) -> Vec<u64> {
    world
        .bus
        .chunks()
        .into_iter()
        .map(|c| {
            serde_json::from_value::<ChunkFrame>(c.frame)
                .expect("frame")
                .seq
        })
        .collect()
}

#[tokio::test]
async fn a_closing_abandoned_while_it_waits_behind_a_chunk_ends_nothing() {
    let world = world();
    let handle = Arc::new(open(&world).await);
    let release = Arc::new(Notify::new());
    world.bus.chunk_mode(ChunkMode::Hold(Arc::clone(&release)));
    let sending = {
        let handle = Arc::clone(&handle);
        tokio::spawn(async move { handle.keepalive().await })
    };
    settle().await;
    let closer = {
        let (client, handle) = (world.client.clone(), Arc::clone(&handle));
        let closing = closing(&handle, world.thread, ClosureKind::Completed);
        tokio::spawn(async move { client.close_message(&closing, Some(&handle)).await })
    };
    settle().await;
    closer.abort();
    let _ = closer.await;
    world.bus.chunk_mode(ChunkMode::Ack);
    release.notify_waiters();
    sending.await.expect("task").expect("acknowledged");
    handle
        .keepalive()
        .await
        .expect("the abandoned closing ended nothing");
    assert_eq!(world.bus.commands().len(), 1, "only the opening was sent");
    assert_eq!(seqs(&world), [0, 1]);
}

#[tokio::test]
async fn a_chunk_operation_abandoned_before_it_took_a_number_changes_nothing() {
    let world = world();
    let handle = Arc::new(open(&world).await);
    let release = Arc::new(Notify::new());
    world.bus.chunk_mode(ChunkMode::Hold(Arc::clone(&release)));
    let first = {
        let handle = Arc::clone(&handle);
        tokio::spawn(async move { handle.keepalive().await })
    };
    settle().await;
    let queued = {
        let handle = Arc::clone(&handle);
        tokio::spawn(async move { handle.keepalive().await })
    };
    settle().await;
    queued.abort();
    let _ = queued.await;
    world.bus.chunk_mode(ChunkMode::Ack);
    release.notify_waiters();
    first.await.expect("task").expect("acknowledged");
    handle.keepalive().await.expect("the handle is not broken");
    assert_eq!(
        seqs(&world),
        [0, 1],
        "no number was spent by the abandoned operation"
    );
}

#[tokio::test]
async fn dropping_a_handle_a_writer_or_a_sink_sends_nothing() {
    let world = world();
    let handle = open(&world).await;
    let writer = handle.writer();
    let factory = handle.sink_factory();
    let sink = factory.sink();
    drop(sink);
    drop(factory);
    drop(writer);
    drop(handle);
    settle().await;
    assert_eq!(world.bus.commands().len(), 1, "only the opening was sent");
    assert!(world.bus.chunks().is_empty());
}
