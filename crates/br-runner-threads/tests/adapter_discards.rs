mod support;

use std::sync::Arc;

use br_llm_messages::DeltaOp;
use br_runner_threads::MemoryQueueStore;
use support::adapter_wire::*;
use support::adapter_x::*;
use support::*;

async fn plain_rig() -> Rig {
    rig_with_store(Arc::new(MemoryQueueStore::new())).await
}

fn is_interruption(delta: &br_llm_messages::ResponseDelta) -> bool {
    matches!(delta.op(), DeltaOp::ResponseInterrupted { .. })
}

#[tokio::test]
async fn discarding_a_response_whose_sink_queued_a_delta_queues_one_delta_that_interrupts_it() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let entry = entry_id();
    let mut sink = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    sink.send(started(turn.id, entry)).await;

    rig.access()
        .discard_response(&thread, entry)
        .await
        .expect("accepted");

    eventually("two chunks", || rig.threads.chunks().len() == 2).await;
    let deltas = deltas_of(&rig.threads, turn.id.as_uuid());
    assert!(is_interruption(&deltas[1]));
    assert_eq!(deltas[1].entry(), entry);
    assert_eq!(deltas[1].turn(), turn.id);
}

#[tokio::test]
async fn discarding_a_response_whose_sink_queued_no_delta_queues_nothing() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let entry = entry_id();
    rig.access()
        .open_response(&thread, turn_ref(rig.agent), entry)
        .await
        .expect("accepted");
    rig.idle().await;

    rig.access()
        .discard_response(&thread, entry)
        .await
        .expect("accepted");

    assert!(rig.adapter.queue_state().threads.is_empty());
    assert!(rig.threads.chunks().is_empty());
}

#[tokio::test]
async fn discarding_a_response_that_is_not_open_is_accepted_and_changes_nothing() {
    let rig = plain_rig().await;

    rig.access()
        .discard_response(&thread_ref(), entry_id())
        .await
        .expect("accepted");

    assert!(rig.adapter.queue_state().threads.is_empty());
    assert!(rig.threads.operations().is_empty());
}

#[tokio::test]
async fn a_discarded_response_ends_its_sink_and_a_second_discard_changes_nothing() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let entry = entry_id();
    let mut sink = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    sink.send(started(turn.id, entry)).await;
    rig.access()
        .discard_response(&thread, entry)
        .await
        .expect("accepted");
    rig.idle().await;
    let before = rig.threads.chunks().len();

    sink.send(inserted(turn.id, entry, block_id(), "late"))
        .await;
    rig.access()
        .discard_response(&thread, entry)
        .await
        .expect("accepted");

    rig.idle().await;
    assert_eq!(rig.threads.chunks().len(), before);
}
