use std::time::Duration;

use br_agentic_harness_contract::{BoxId, FilePath, SandboxPort, Seed, SeedEntry};
use br_runner_sandbox::PodKeys;
use serde_json::json;

use crate::fakes::{
    Boxes, Lineage, Script, boxes, boxes_with, fresh_request, request, settle, warm_pod,
};

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

async fn dropped_while_parked(boxes: &Boxes, open: &br_agentic_harness_contract::OpenRequest) {
    let dropped = tokio::time::timeout(Duration::from_secs(1), boxes.sandbox.open(open)).await;
    settle().await;
    assert!(dropped.is_err());
}

fn clean_of(boxes: &Boxes, pod: &str) -> Option<String> {
    boxes
        .cluster
        .pod(pod)
        .and_then(|pod| pod.annotations.get(&PodKeys::default().clean()).cloned())
}

#[tokio::test(start_paused = true)]
async fn dropped_open_while_the_live_claim_is_in_flight_the_instance_is_free_and_the_pod_is_left_with_clean_false()
 {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    boxes
        .cluster
        .add_pod(crate::fakes::live_pod("live-1", &lineage, parent));
    boxes.cluster.gate.park_after_patch();
    let open = fresh_request(lineage.after(parent));

    dropped_while_parked(&boxes, &open).await;

    let patched: Vec<String> = boxes
        .cluster
        .patches()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(patched, vec!["live-1".to_owned()]);
    assert!(boxes.cluster.deleted().is_empty());
    assert_eq!(clean_of(&boxes, "live-1").as_deref(), Some("false"));
    boxes.cluster.gate.open();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    settle().await;
    let again = boxes.sandbox.open(&open).await.expect("an open box");
    assert_eq!(again.box_id, BoxId::new("warm-main-a"));
}

#[tokio::test(start_paused = true)]
async fn dropped_open_while_the_warm_claim_is_in_flight_the_instance_is_free_and_the_pod_is_left_with_clean_false()
 {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.gate.park_after_patch();
    let open = fresh_request(Lineage::new().first());

    dropped_while_parked(&boxes, &open).await;

    assert_eq!(boxes.cluster.patches().len(), 1);
    assert!(boxes.cluster.deleted().is_empty());
    assert_eq!(clean_of(&boxes, "warm-main-a").as_deref(), Some("false"));
    boxes.cluster.gate.open();
    boxes.cluster.add_pod(warm_pod("warm-main-b"));
    settle().await;
    let again = boxes.sandbox.open(&open).await.expect("an open box");
    assert_eq!(again.box_id, BoxId::new("warm-main-b"));
}

#[tokio::test(start_paused = true)]
async fn dropped_open_while_the_cold_create_is_in_flight_the_instance_is_free_and_the_pod_is_left_with_clean_false()
 {
    let boxes = boxes();
    boxes.cluster.gate.park_after_create();
    let open = fresh_request(Lineage::new().first());

    dropped_while_parked(&boxes, &open).await;

    assert_eq!(
        boxes.cluster.pod_names(),
        vec!["warm-main-cold-1".to_owned()]
    );
    assert!(boxes.cluster.deleted().is_empty());
    assert_eq!(
        clean_of(&boxes, "warm-main-cold-1").as_deref(),
        Some("false")
    );
    boxes.cluster.gate.open();
    let again = boxes.sandbox.open(&open).await.expect("an open box");
    assert_eq!(again.box_id, BoxId::new("warm-main-cold-2"));
}
