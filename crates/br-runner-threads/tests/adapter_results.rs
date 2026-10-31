mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use br_runner_threads::MemoryQueueStore;
use support::adapter_wire::*;
use support::adapter_x::*;
use support::*;

async fn plain_rig() -> Rig {
    rig_with_store(Arc::new(MemoryQueueStore::new())).await
}

#[tokio::test]
async fn opening_a_result_on_a_new_turn_queues_only_the_opening_and_drops_the_fragments() {
    let rig = plain_rig().await;
    let turn = turn_ref(rig.agent);

    let mut stream = rig
        .access()
        .open_result(&thread_ref(), turn.clone(), entry_id())
        .await
        .expect("accepted");
    stream.send("a fragment".to_owned()).await;
    stream.send("another fragment".to_owned()).await;

    eventually("the opening", || rig.threads.commands().len() == 1).await;
    rig.idle().await;
    assert_eq!(stores(&rig.threads).len(), 1);
    assert!(is_streaming(&stores(&rig.threads)[0]));
    assert_eq!(stores(&rig.threads)[0].message_id, turn.id.as_uuid());
    assert!(rig.threads.chunks().is_empty());
}

#[tokio::test]
async fn opening_a_result_on_an_open_turn_queues_nothing() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    rig.access()
        .add_entry(&thread, turn.clone(), notice("before"))
        .await
        .expect("accepted");
    rig.idle().await;
    let before = rig.threads.operations().len();

    rig.access()
        .open_result(&thread, turn, entry_id())
        .await
        .expect("accepted");

    rig.idle().await;
    assert_eq!(rig.threads.operations().len(), before);
}

#[tokio::test]
async fn discarding_a_result_queues_nothing() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let entry = entry_id();
    rig.access()
        .open_result(&thread, turn_ref(rig.agent), entry)
        .await
        .expect("accepted");
    rig.idle().await;
    let before = rig.threads.operations().len();

    rig.access()
        .discard_result(&thread, entry)
        .await
        .expect("accepted");

    rig.idle().await;
    assert_eq!(rig.threads.operations().len(), before);
}

#[tokio::test]
async fn the_write_context_is_asked_once_per_operation_that_queues_and_never_for_a_delta_or_a_discard()
 {
    let calls = Arc::new(AtomicUsize::new(0));
    let agent = agent_id();
    let rig = rig_over(
        Arc::new(MemoryQueueStore::new()),
        counting_context(agent, Arc::clone(&calls)),
        agent,
    )
    .await;
    let thread = thread_ref();
    let turn = turn_ref(agent);
    let (entry, block) = (entry_id(), block_id());

    let mut sink = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    let after_opening = calls.load(Ordering::SeqCst);
    sink.send(started(turn.id, entry)).await;
    sink.send(inserted(turn.id, entry, block, "text")).await;
    let after_deltas = calls.load(Ordering::SeqCst);
    rig.access()
        .complete_response(&thread, turn.clone(), text_response(entry, block, "text"))
        .await
        .expect("accepted");
    let after_completion = calls.load(Ordering::SeqCst);
    rig.access()
        .discard_response(&thread, entry_id())
        .await
        .expect("accepted");
    let after_discard = calls.load(Ordering::SeqCst);
    rig.access()
        .add_entry(&thread, turn, notice("more"))
        .await
        .expect("accepted");

    assert_eq!(after_opening, 1);
    assert_eq!(after_deltas, 1);
    assert_eq!(after_completion, 2);
    assert_eq!(after_discard, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}
