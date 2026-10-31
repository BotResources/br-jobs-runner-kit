mod support;

use br_runner_threads::{ItemKind, ReportKind};
use support::adapter_x::*;
use support::gate_store::GateStore;
use support::*;

#[tokio::test]
async fn an_append_in_flight_when_the_adapter_is_dropped_ends_unsent_and_starts_no_delivery() {
    let store = GateStore::after_free_appends(1);
    let agent = agent_id();
    let rig = rig_over(store.clone(), fixed_context(context_of(agent)), agent).await;
    let turn = turn_ref(agent);
    let entry = entry_id();
    let mut sink = rig
        .access()
        .open_response(&thread_ref(), turn.clone(), entry)
        .await
        .expect("accepted");
    rig.idle().await;
    let operations_before = rig.threads.operations().len();

    let sending = tokio::spawn(async move {
        sink.send(support::adapter_wire::started(turn.id, entry))
            .await;
    });
    store.a_gated_append_started().await;
    let Rig {
        threads,
        recorder,
        adapter,
        ..
    } = rig;
    drop(adapter);
    store.release_appends();
    sending.await.expect("the sink task ends");

    eventually("the unsent report", || {
        recorder
            .reports()
            .iter()
            .any(|report| report.kind == ReportKind::Unsent && report.item_kind == ItemKind::Chunk)
    })
    .await;
    settle().await;
    assert_eq!(threads.operations().len(), operations_before);
    assert_eq!(store.kept().await.len(), 1);
}
