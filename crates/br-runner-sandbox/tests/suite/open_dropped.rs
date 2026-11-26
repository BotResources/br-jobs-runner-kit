use std::time::Duration;

use br_agentic_harness_contract::{BoxId, FilePath, SandboxPort, Seed, SeedEntry};
use serde_json::json;

use crate::fakes::{Lineage, Script, boxes, boxes_with, fresh_request, request, settle, warm_pod};

fn seeded(lineage: &Lineage) -> br_agentic_harness_contract::OpenRequest {
    let entry = SeedEntry::file(FilePath::new("a.md").expect("a path"), 0o644, b"x".to_vec());
    request(lineage.first(), Seed::new(vec![entry]).expect("a seed"))
}

#[tokio::test(start_paused = true)]
async fn dropped_open_after_the_claim_the_instance_is_free_and_the_pod_of_the_first_call_is_gone() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.scripts.push_script(Script::hanging());
    let lineage = Lineage::new();
    let first = seeded(&lineage);

    let dropped = tokio::time::timeout(Duration::from_secs(1), boxes.sandbox.open(&first)).await;
    assert!(dropped.is_err());
    settle().await;

    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-a".to_owned()]);
    boxes.cluster.add_pod(warm_pod("warm-main-b"));
    settle().await;
    boxes
        .cluster
        .scripts
        .push_script(Script::like_bsdtar_extract());
    let again = boxes.sandbox.open(&first).await.expect("an open box");
    assert_eq!(again.box_id, BoxId::new("warm-main-b"));
}

#[tokio::test(start_paused = true)]
async fn dropped_open_the_dropped_call_leaves_no_box_in_the_adapter() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.scripts.push_script(Script::hanging());
    let lineage = Lineage::new();

    let dropped = tokio::time::timeout(
        Duration::from_secs(1),
        boxes.sandbox.open(&seeded(&lineage)),
    )
    .await;
    settle().await;

    assert!(dropped.is_err());
    let error = boxes
        .sandbox
        .release(&BoxId::new("warm-main-a"))
        .await
        .expect_err("not a box of this adapter");
    assert!(!error.is_refusal());
}

#[tokio::test(start_paused = true)]
async fn dropped_open_while_the_cold_pod_starts_the_cold_pod_is_deleted_and_the_instance_is_free() {
    let boxes = boxes_with(json!({"open_timeout_seconds": 30}));
    boxes.cluster.ready_on_create(false);
    let open = fresh_request(Lineage::new().first());

    let dropped = tokio::time::timeout(Duration::from_secs(1), boxes.sandbox.open(&open)).await;
    settle().await;

    assert!(dropped.is_err());
    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-cold-1".to_owned()]);
    boxes.cluster.ready_on_create(true);
    let again = boxes.sandbox.open(&open).await.expect("an open box");
    assert_eq!(again.box_id, BoxId::new("warm-main-cold-2"));
}

#[tokio::test(start_paused = true)]
async fn dropped_open_during_the_restore_the_claimed_pod_is_deleted_and_the_instance_is_free() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    crate::fakes::staged_save(&boxes, &lineage, parent);
    boxes.cluster.scripts.push_script(Script::hanging());
    let open = fresh_request(lineage.after(parent));

    let dropped = tokio::time::timeout(Duration::from_secs(1), boxes.sandbox.open(&open)).await;
    settle().await;

    assert!(dropped.is_err());
    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-a".to_owned()]);
}

#[tokio::test(start_paused = true)]
async fn dropped_open_after_a_live_claim_the_reused_pod_is_deleted() {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    boxes
        .cluster
        .add_pod(crate::fakes::live_pod("live-1", &lineage, parent));
    let open = fresh_request(lineage.after(parent));

    let first = boxes.sandbox.open(&open).await.expect("a box");
    boxes
        .sandbox
        .release(&first.box_id)
        .await
        .expect("released");
    boxes
        .cluster
        .add_pod(crate::fakes::live_pod("live-2", &lineage, parent));
    settle().await;
    let future = boxes.sandbox.open(&open);
    drop(future);
    settle().await;

    assert!(boxes.sandbox.open(&open).await.is_ok());
}
