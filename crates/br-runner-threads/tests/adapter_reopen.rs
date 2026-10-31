mod support;

use std::sync::Arc;

use br_llm_messages::DeltaOp;
use br_runner_threads::{Error, MemoryQueueStore};
use support::adapter_wire::*;
use support::adapter_x::*;
use support::*;

fn is_interruption(delta: &br_llm_messages::ResponseDelta) -> bool {
    matches!(delta.op(), DeltaOp::ResponseInterrupted { .. })
}

async fn plain_rig() -> Rig {
    rig_with_store(Arc::new(MemoryQueueStore::new())).await
}

#[tokio::test]
async fn opening_a_response_again_ends_the_earlier_one_as_interrupted_and_leaves_its_sink_dead() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let entry = entry_id();
    let mut earlier = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    earlier.send(started(turn.id, entry)).await;

    let mut later = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    earlier
        .send(inserted(
            turn.id,
            entry,
            block_id(),
            "from the earlier sink",
        ))
        .await;
    later.send(started(turn.id, entry)).await;

    eventually("three chunks", || rig.threads.chunks().len() == 3).await;
    rig.idle().await;
    let deltas = deltas_of(&rig.threads, turn.id.as_uuid());
    assert_eq!(deltas.len(), 3);
    assert!(is_interruption(&deltas[1]));
    assert!(!is_interruption(&deltas[2]));
    assert_eq!(stores(&rig.threads).len(), 1);
}

#[tokio::test]
async fn opening_a_response_again_after_no_delta_queues_nothing_for_the_earlier_one() {
    let rig = plain_rig().await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let entry = entry_id();
    rig.access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");

    rig.access()
        .open_response(&thread, turn, entry)
        .await
        .expect("accepted");

    rig.idle().await;
    assert!(rig.threads.chunks().is_empty());
    assert_eq!(stores(&rig.threads).len(), 1);
}

#[tokio::test]
async fn opening_again_is_refused_with_store_when_the_end_cannot_be_kept_and_the_earlier_stays_open()
 {
    let flaky = FlakyStore::new();
    let rig = rig_with_store(flaky.clone()).await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let entry = entry_id();
    let mut earlier = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    earlier.send(started(turn.id, entry)).await;
    rig.idle().await;

    flaky.refuse_appends_from_now();
    let refused = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await;
    flaky.accept_appends_again();
    rig.access()
        .discard_response(&thread, entry)
        .await
        .expect("accepted");

    assert!(matches!(fault_of(refused), Error::Store(_)));
    eventually("the earlier response ends", || {
        deltas_of(&rig.threads, turn.id.as_uuid())
            .last()
            .is_some_and(is_interruption)
    })
    .await;
}

#[tokio::test]
async fn a_failed_append_for_a_delta_stops_the_sink_and_refuses_the_completion_with_store() {
    let flaky = FlakyStore::new();
    let rig = rig_with_store(flaky.clone()).await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let (entry, block) = (entry_id(), block_id());
    let mut sink = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    sink.send(started(turn.id, entry)).await;

    flaky.refuse_appends_from_now();
    sink.send(inserted(turn.id, entry, block, "lost")).await;
    flaky.accept_appends_again();
    sink.send(inserted(turn.id, entry, block, "after the failure"))
        .await;
    let refused = rig
        .access()
        .complete_response(&thread, turn.clone(), text_response(entry, block, "lost"))
        .await;

    assert!(matches!(fault_of(refused), Error::Store(_)));
    rig.idle().await;
    assert_eq!(rig.threads.chunks().len(), 1);
}

#[tokio::test]
async fn a_discard_after_a_refused_completion_still_queues_the_end_of_the_response() {
    let flaky = FlakyStore::new();
    let rig = rig_with_store(flaky.clone()).await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let (entry, block) = (entry_id(), block_id());
    let mut sink = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    sink.send(started(turn.id, entry)).await;
    flaky.accept_only(1);
    let refused = rig
        .access()
        .complete_response(&thread, turn.clone(), text_response(entry, block, "answer"))
        .await;
    flaky.accept_appends_again();

    rig.access()
        .discard_response(&thread, entry)
        .await
        .expect("accepted");

    assert!(refused.is_err());
    eventually("the ending", || {
        deltas_of(&rig.threads, turn.id.as_uuid())
            .last()
            .is_some_and(is_interruption)
    })
    .await;
}

#[tokio::test]
async fn a_discard_is_refused_with_store_when_the_end_cannot_be_kept() {
    let flaky = FlakyStore::new();
    let rig = rig_with_store(flaky.clone()).await;
    let thread = thread_ref();
    let turn = turn_ref(rig.agent);
    let entry = entry_id();
    let mut sink = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    sink.send(started(turn.id, entry)).await;
    flaky.refuse_appends_from_now();

    let refused = rig.access().discard_response(&thread, entry).await;

    assert!(matches!(fault_of(refused), Error::Store(_)));
}
