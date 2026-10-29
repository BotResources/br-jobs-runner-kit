mod support;

use std::sync::Arc;

use br_runner_threads::ClosureKind;
use contract_threads::{Chunk, ChunkFrame};
use support::*;

fn frames(world: &World) -> Vec<ChunkFrame> {
    world
        .bus
        .chunks()
        .into_iter()
        .map(|c| serde_json::from_value(c.frame).expect("a frame of the contract"))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writers_the_handle_and_sinks_used_at_once_leave_one_gapless_sequence_and_a_seal_over_all_of_it()
 {
    const WRITES: usize = 90;
    let world = world();
    let handle = Arc::new(open(&world).await);
    let message = handle.message_id();
    let writer = handle.writer();
    let factory = handle.sink_factory();
    let mut tasks = Vec::new();
    for index in 0..WRITES {
        let handle = Arc::clone(&handle);
        let writer = writer.clone();
        let mut sink = factory.sink();
        tasks.push(tokio::spawn(async move {
            match index % 4 {
                0 => handle.keepalive().await,
                1 => writer.entry(&notice()).await,
                2 => sink.send(delta_for(message)).await,
                _ => writer.keepalive().await,
            }
        }));
    }
    for result in futures::future::join_all(tasks).await {
        result.expect("task").expect("write");
    }
    let frames = frames(&world);
    assert_eq!(
        frames.iter().map(|f| f.seq).collect::<Vec<_>>(),
        (0..WRITES as u64).collect::<Vec<_>>()
    );
    let closing = closing(&handle, world.thread, ClosureKind::Completed);
    world
        .client
        .close_message(&closing, Some(&handle))
        .await
        .expect("closed");
    let sent = world.bus.commands().pop().expect("the closing");
    let chunks: Vec<Chunk> = frames.into_iter().map(|f| f.chunk).collect();
    assert_eq!(sent.payload["seal"]["last_seq"], WRITES as u64 - 1);
    assert_eq!(
        sent.payload["seal"]["hash"],
        serde_json::json!(contract_threads::seal_hash(chunks.iter()).expect("hash"))
    );
}
