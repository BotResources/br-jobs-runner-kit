mod support;

use std::time::Duration;

use br_agentic_contract::ThreadAccess;
use br_runner_threads::EntryOutcome;
use support::adapter_wire::*;
use support::adapter_x::*;
use support::gate_store::GateStore;
use support::queue_kit::pends;
use support::*;

const SHORT: Duration = Duration::from_millis(50);

async fn dropped<T>(future: impl std::future::Future<Output = T>) {
    let _ = tokio::time::timeout(SHORT, future).await;
}

#[tokio::test]
async fn a_port_call_dropped_while_its_append_is_in_flight_ends_as_if_it_had_been_awaited() {
    let store = GateStore::after_free_appends(1);
    let agent = agent_id();
    let rig = rig_over(store.clone(), fixed_context(context_of(agent)), agent).await;
    let (thread, turn) = (thread_ref(), turn_ref(agent));
    let first = notice("first");
    let first_entry = first.id();

    dropped(rig.access().add_entry(&thread, turn.clone(), first)).await;
    store.release_appends();
    rig.access()
        .add_entry(&thread, turn.clone(), notice("second"))
        .await
        .expect("accepted");

    rig.idle().await;
    let seqs: Vec<u64> = chunks_of(&rig.threads, turn.id.as_uuid())
        .iter()
        .map(|(seq, _)| *seq)
        .collect();
    assert_eq!(seqs, [0, 1]);
    assert_eq!(
        entry_of(&chunks_of(&rig.threads, turn.id.as_uuid())[0].1).id(),
        first_entry
    );
}

#[tokio::test]
async fn entry_stored_waits_for_the_entry_of_a_dropped_call_until_it_is_delivered() {
    let store = GateStore::after_free_appends(1);
    let agent = agent_id();
    let rig = rig_over(store.clone(), fixed_context(context_of(agent)), agent).await;
    rig.threads.pause();
    let (thread, turn) = (thread_ref(), turn_ref(agent));
    let first = notice("first");
    let first_entry = first.id();
    dropped(rig.access().add_entry(&thread, turn, first)).await;
    store.release_appends();
    eventually("the item is queued", || {
        rig.adapter
            .queue_state()
            .threads
            .first()
            .is_some_and(|queue| queue.waiting == 2)
    })
    .await;

    let early = pends(rig.adapter.entry_stored(&thread, first_entry)).await;
    rig.threads.resume();
    let late = rig.adapter.entry_stored(&thread, first_entry).await;

    assert!(
        early,
        "entry_stored answered before the entry was delivered"
    );
    assert_eq!(late, EntryOutcome::Stored);
}

#[tokio::test]
async fn a_later_operation_on_the_thread_waits_behind_the_detached_one() {
    let store = GateStore::after_free_appends(1);
    let agent = agent_id();
    let rig = rig_over(store.clone(), fixed_context(context_of(agent)), agent).await;
    let (thread, turn) = (thread_ref(), turn_ref(agent));
    dropped(
        rig.access()
            .add_entry(&thread, turn.clone(), notice("first")),
    )
    .await;
    let later = notice("later");
    let later_entry = later.id();

    let (result, sent_while_waiting) = tokio::join!(
        rig.access().add_entry(&thread, turn.clone(), later),
        async {
            settle().await;
            let sent = rig.threads.chunks().len() + store.kept().await.len();
            store.release_appends();
            sent
        }
    );

    result.expect("accepted");
    assert_eq!(sent_while_waiting, 0, "the later call went past the first");
    rig.idle().await;
    let chunks = chunks_of(&rig.threads, turn.id.as_uuid());
    assert_eq!(chunks.len(), 2);
    assert_eq!(entry_of(&chunks[1].1).id(), later_entry);
}

#[tokio::test]
async fn a_sink_delta_dropped_while_its_append_is_in_flight_still_counts_as_sent() {
    let store = GateStore::after_free_appends(1);
    let agent = agent_id();
    let rig = rig_over(store.clone(), fixed_context(context_of(agent)), agent).await;
    let (thread, turn, entry) = (thread_ref(), turn_ref(agent), entry_id());
    let mut sink = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");

    dropped(sink.send(started(turn.id, entry))).await;
    store.release_appends();
    rig.access()
        .discard_response(&thread, entry)
        .await
        .expect("accepted");

    rig.idle().await;
    let deltas = deltas_of(&rig.threads, turn.id.as_uuid());
    assert_eq!(
        deltas.len(),
        2,
        "the delta and the interruption: {deltas:?}"
    );
}

#[tokio::test]
async fn a_sink_delta_of_a_response_that_ended_while_it_waited_does_nothing() {
    let store = GateStore::after_free_appends(1);
    let agent = agent_id();
    let rig = rig_over(store.clone(), fixed_context(context_of(agent)), agent).await;
    let thread = thread_ref();
    let (turn, entry) = (turn_ref(agent), entry_id());
    let mut sink = rig
        .access()
        .open_response(&thread, turn.clone(), entry)
        .await
        .expect("accepted");
    let response = text_response(entry, block_id(), "the answer");
    dropped(
        rig.access()
            .add_entry(&thread, turn_ref(agent), notice("other turn")),
    )
    .await;

    let late = started(turn.id, entry);
    tokio::join!(
        async {
            rig.access()
                .complete_response(&thread, turn.clone(), response.clone())
                .await
                .expect("accepted");
        },
        sink.send(late),
        async {
            settle().await;
            store.release_appends();
        }
    );

    rig.idle().await;
    let deltas = deltas_of(&rig.threads, turn.id.as_uuid());
    assert_eq!(folded(&deltas, turn.id, entry), Some(response));
    assert_eq!(deltas.len(), 3, "{deltas:?}");
}
