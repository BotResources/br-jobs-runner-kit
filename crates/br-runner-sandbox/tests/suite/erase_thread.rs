use br_agentic_harness_contract::{FreshReason, PortError, SandboxPort, StartKind};
use br_runner_sandbox::{
    AttemptId, Record, SaveKey, Tombstone, manifest_key, record_key, tombstone_key,
};

use crate::fakes::{
    Boxes, Lineage, boxes, fresh_request, live_pod, staged_save, start_time, warm_pod,
};

fn put_record_and_tombstone(
    boxes: &Boxes,
    lineage: &Lineage,
    turn: br_agentic_harness_contract::TurnId,
) {
    let key = SaveKey::new(
        boxes.config.client_id().clone(),
        lineage.thread,
        lineage.agent,
        turn,
    );
    boxes.store.insert(
        &record_key(&key),
        Record::started(AttemptId::mint(), start_time())
            .to_vec()
            .expect("bytes"),
    );
    boxes.store.insert(
        &tombstone_key(&key),
        Tombstone::new(turn, start_time(), start_time())
            .to_vec()
            .expect("bytes"),
    );
}

#[tokio::test(start_paused = true)]
async fn erase_deletes_the_three_prefixes_of_the_thread_and_nothing_of_another_thread() {
    let boxes = boxes();
    let (mine, other) = (Lineage::new(), Lineage::new());
    let turn = crate::common::turn();
    staged_save(&boxes, &mine, turn);
    put_record_and_tombstone(&boxes, &mine, turn);
    let kept = staged_save(&boxes, &other, turn);
    put_record_and_tombstone(&boxes, &other, turn);

    boxes.sandbox.erase(mine.thread).await.expect("erased");

    let keys = boxes.store.keys();
    assert!(
        keys.iter()
            .all(|key| !key.contains(&mine.thread.to_string())),
        "{keys:?}"
    );
    assert!(keys.contains(&manifest_key(kept.key())));
    assert_eq!(keys.len(), 4);
}

#[tokio::test(start_paused = true)]
async fn erase_deletes_the_manifest_before_the_part() {
    let boxes = boxes();
    let lineage = Lineage::new();
    let manifest = staged_save(&boxes, &lineage, crate::common::turn());

    boxes.sandbox.erase(lineage.thread).await.expect("erased");

    let log = boxes.store.log();
    let at = |key: &str| {
        log.iter()
            .position(|line| line == &format!("delete {key}"))
            .expect("deleted")
    };
    assert!(at(&manifest_key(manifest.key())) < at(&manifest.part().key));
}

#[tokio::test(start_paused = true)]
async fn erase_deletes_the_pods_of_the_thread_in_the_namespace_and_no_other_pod() {
    let boxes = boxes();
    let (mine, other) = (Lineage::new(), Lineage::new());
    let turn = crate::common::turn();
    boxes.cluster.add_pod(live_pod("mine-1", &mine, turn));
    boxes.cluster.add_pod(live_pod("mine-2", &mine, turn));
    boxes.cluster.add_pod(live_pod("other-1", &other, turn));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));

    boxes.sandbox.erase(mine.thread).await.expect("erased");

    assert_eq!(
        boxes.cluster.pod_names(),
        vec!["other-1".to_owned(), "warm-main-a".to_owned()]
    );
}

#[tokio::test(start_paused = true)]
async fn after_an_erase_every_open_of_the_thread_is_fresh_with_no_save_ever() {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    staged_save(&boxes, &lineage, parent);
    put_record_and_tombstone(&boxes, &lineage, parent);
    boxes.cluster.add_pod(live_pod("live-1", &lineage, parent));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    crate::fakes::settle().await;

    boxes.sandbox.erase(lineage.thread).await.expect("erased");
    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    assert_eq!(opened.start, StartKind::Fresh(FreshReason::NoSaveEver));
}

#[tokio::test(start_paused = true)]
async fn erase_failures_of_the_store_or_the_cluster_are_port_faults() {
    let failing_store = boxes();
    let lineage = Lineage::new();
    staged_save(&failing_store, &lineage, crate::common::turn());
    failing_store.store.fail("delete", "manifest");
    let error: PortError = failing_store
        .sandbox
        .erase(lineage.thread)
        .await
        .expect_err("a fault");
    assert!(!error.is_refusal());

    let failing_list = boxes();
    failing_list.cluster.fail_lists();
    let error = failing_list
        .sandbox
        .erase(lineage.thread)
        .await
        .expect_err("a fault");
    assert!(!error.is_refusal());
}
