use std::time::Duration;

use br_agentic_harness_contract::{BoxId, SandboxPort};
use br_runner_sandbox::{Cluster, ClusterError, PodKeys};
use serde_json::json;

use crate::fakes::{Boxes, Lineage, Script, boxes, boxes_with, fresh_request, warm_pod};
use crate::part_tar::sample_tar;

async fn saved_box(boxes: &Boxes) -> BoxId {
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let lineage = Lineage::new();
    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.first()))
        .await
        .expect("a box");
    boxes.cluster.scripts.push_script(Script::out(sample_tar()));
    boxes.sandbox.save(&opened.box_id).await.expect("a receipt");
    opened.box_id
}

#[tokio::test(start_paused = true)]
async fn release_a_saved_box_at_or_above_the_threshold_is_kept_with_idle_since_set_at_the_release()
{
    let boxes = boxes_with(json!({"idle_keep_min_state_bytes": 1}));
    let id = saved_box(&boxes).await;
    let keys = PodKeys::default();
    assert!(
        !boxes
            .cluster
            .pod(id.as_str())
            .expect("a pod")
            .annotations
            .contains_key(&keys.idle_since())
    );
    boxes.clock.advance(Duration::from_secs(30));

    boxes.sandbox.release(&id).await.expect("released");

    let pod = boxes.cluster.pod(id.as_str()).expect("kept");
    assert_eq!(
        pod.annotations.get(&keys.idle_since()),
        Some(&boxes.clock_now_text())
    );
    assert!(boxes.cluster.deleted().is_empty());
}

#[tokio::test(start_paused = true)]
async fn release_a_saved_box_below_the_threshold_deletes_the_pod() {
    let boxes = boxes();
    let id = saved_box(&boxes).await;

    boxes.sandbox.release(&id).await.expect("released");

    assert_eq!(boxes.cluster.deleted(), vec![id.to_string()]);
}

#[tokio::test(start_paused = true)]
async fn release_a_box_that_was_not_saved_deletes_the_pod() {
    let boxes = boxes_with(json!({"idle_keep_min_state_bytes": 1}));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");

    boxes
        .sandbox
        .release(&opened.box_id)
        .await
        .expect("released");

    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-a".to_owned()]);
}

#[tokio::test(start_paused = true)]
async fn release_a_box_whose_save_failed_deletes_the_pod() {
    let boxes = boxes_with(json!({"idle_keep_min_state_bytes": 1}));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");
    boxes.cluster.scripts.push_script(Script::exit(2));
    boxes
        .sandbox
        .save(&opened.box_id)
        .await
        .expect_err("a fault");

    boxes
        .sandbox
        .release(&opened.box_id)
        .await
        .expect("released");

    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-a".to_owned()]);
}

#[tokio::test(start_paused = true)]
async fn release_a_save_whose_pod_patch_failed_is_not_marked_saved_and_release_deletes_the_pod() {
    let boxes = boxes_with(json!({"idle_keep_min_state_bytes": 1}));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");
    boxes
        .cluster
        .fail_patches_touching(&PodKeys::default().state_turn());
    boxes.cluster.scripts.push_script(Script::out(sample_tar()));

    let receipt = boxes.sandbox.save(&opened.box_id).await;
    boxes
        .sandbox
        .release(&opened.box_id)
        .await
        .expect("released");

    assert!(receipt.is_ok());
    assert_eq!(boxes.cluster.deleted(), vec!["warm-main-a".to_owned()]);
}

#[tokio::test(start_paused = true)]
async fn release_of_a_pod_that_is_already_gone_is_a_success_after_a_save_and_with_no_save() {
    let kept = boxes_with(json!({"idle_keep_min_state_bytes": 1}));
    let saved = saved_box(&kept).await;
    kept.cluster
        .delete_pod(saved.as_str())
        .await
        .expect("deleted");
    kept.sandbox.release(&saved).await.expect("a success");

    let plain = boxes();
    plain.cluster.add_pod(warm_pod("warm-main-a"));
    let opened = plain
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");
    plain
        .cluster
        .delete_pod("warm-main-a")
        .await
        .expect("deleted");
    plain
        .sandbox
        .release(&opened.box_id)
        .await
        .expect("a success");
}

#[tokio::test(start_paused = true)]
async fn release_twice_is_accepted_and_the_second_call_does_nothing() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");
    boxes.sandbox.release(&opened.box_id).await.expect("first");
    let calls = boxes.cluster.calls().len();

    boxes.sandbox.release(&opened.box_id).await.expect("second");

    assert_eq!(boxes.cluster.calls().len(), calls);
}

#[tokio::test(start_paused = true)]
async fn release_of_a_box_that_was_never_opened_is_a_port_fault() {
    let boxes = boxes();

    let error = boxes
        .sandbox
        .release(&BoxId::new("warm-main-zzz"))
        .await
        .expect_err("a fault");

    assert!(!error.is_refusal());
}

#[tokio::test(start_paused = true)]
async fn release_a_pod_that_cannot_be_deleted_is_a_port_fault_and_the_box_is_still_released() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let request = fresh_request(Lineage::new().first());
    let opened = boxes.sandbox.open(&request).await.expect("a box");
    boxes
        .cluster
        .fail_deletes(ClusterError::Call("the API server refuses".to_owned()));

    let error = boxes
        .sandbox
        .release(&opened.box_id)
        .await
        .expect_err("a fault");

    assert!(!error.is_refusal());
    let after = boxes
        .sandbox
        .save(&opened.box_id)
        .await
        .expect_err("released");
    assert!(!after.is_refusal());
    boxes.cluster.add_pod(warm_pod("warm-main-b"));
    assert!(boxes.sandbox.open(&request).await.is_ok());
}

#[tokio::test(start_paused = true)]
async fn release_never_writes_a_tombstone_never_deletes_a_save_and_never_reads_the_store() {
    let boxes = boxes();
    let id = saved_box(&boxes).await;
    boxes.store.clear_log();

    boxes.sandbox.release(&id).await.expect("released");

    assert!(boxes.store.log().is_empty());
    assert!(
        boxes
            .store
            .keys()
            .iter()
            .all(|key| !key.starts_with("tombstones/"))
    );
}
