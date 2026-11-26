use br_agentic_harness_contract::{BoxId, OpenFault, PortError, SandboxPort};
use br_runner_sandbox::{Cluster, PodKeys};
use serde_json::json;

use crate::fakes::{Lineage, boxes, boxes_with, failing, fresh_request, pool, settle, warm_pod};

fn not_ready(name: &str) -> br_runner_sandbox::PodView {
    let mut pod = warm_pod(name);
    pod.ready_condition = false;
    pod
}

#[tokio::test(start_paused = true)]
async fn warm_pods_a_pod_added_after_the_adapter_was_built_is_known_from_the_watch() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let lineage = Lineage::new();
    let first = boxes
        .sandbox
        .open(&fresh_request(lineage.first()))
        .await
        .expect("a box");

    boxes.cluster.add_pod(warm_pod("warm-main-b"));
    settle().await;
    let second = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");

    assert_eq!(first.box_id, BoxId::new("warm-main-a"));
    assert_eq!(second.box_id, BoxId::new("warm-main-b"));
}

#[tokio::test(start_paused = true)]
async fn warm_pods_a_pod_deleted_is_no_longer_taken_and_no_open_lists_the_warm_pods() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.add_pod(warm_pod("warm-main-b"));
    settle().await;
    boxes
        .cluster
        .delete_pod("warm-main-a")
        .await
        .expect("deleted");
    settle().await;

    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");

    assert_eq!(opened.box_id, BoxId::new("warm-main-b"));
    assert!(
        boxes
            .cluster
            .calls()
            .iter()
            .all(|call| !call.starts_with("list "))
    );
}

#[tokio::test(start_paused = true)]
async fn warm_pods_only_a_running_ready_pod_with_no_deletion_time_is_ready() {
    let boxes = boxes();
    let mut pending = warm_pod("warm-main-a");
    pending.phase = Some("Pending".to_owned());
    let mut deleting = warm_pod("warm-main-b");
    deleting.deleting = true;
    boxes.cluster.add_pod(pending);
    boxes.cluster.add_pod(deleting);
    boxes.cluster.add_pod(not_ready("warm-main-c"));
    boxes.cluster.add_pod(warm_pod("warm-main-d"));

    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");

    assert_eq!(opened.box_id, BoxId::new("warm-main-d"));
}

#[tokio::test(start_paused = true)]
async fn warm_pods_a_pod_of_another_pool_or_a_claimed_pod_is_never_taken() {
    let boxes = boxes();
    let keys = PodKeys::default();
    let mut other_pool = warm_pod("warm-other-a");
    other_pool.labels.insert(keys.pool_id(), "other".to_owned());
    let mut claimed = warm_pod("warm-main-a");
    claimed.labels.insert(keys.pool(), "claimed".to_owned());
    boxes.cluster.add_pod(other_pool);
    boxes.cluster.add_pod(claimed);
    boxes.cluster.add_pod(warm_pod("warm-main-b"));

    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");

    assert_eq!(opened.box_id, BoxId::new("warm-main-b"));
}

#[tokio::test(start_paused = true)]
async fn warm_claim_a_conflict_or_a_missing_pod_makes_the_adapter_take_the_next_pod() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.add_pod(warm_pod("warm-main-b"));
    boxes.cluster.add_pod(warm_pod("warm-main-c"));
    settle().await;
    boxes.cluster.rival_touches("warm-main-a");
    boxes
        .cluster
        .delete_pod("warm-main-b")
        .await
        .expect("deleted");

    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");

    assert_eq!(opened.box_id, BoxId::new("warm-main-c"));
    let keys = PodKeys::default();
    let a = boxes.cluster.pod("warm-main-a").expect("a pod");
    assert_eq!(a.labels.get(&keys.pool()).map(String::as_str), Some("warm"));
}

#[tokio::test(start_paused = true)]
async fn warm_claim_after_the_maximum_of_failed_tries_the_adapter_goes_to_the_cold_path() {
    let boxes = boxes_with(json!({"claim_max_attempts": 2}));
    for name in ["warm-main-a", "warm-main-b", "warm-main-c"] {
        boxes.cluster.add_pod(warm_pod(name));
        boxes.cluster.rival_touches(name);
    }

    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");

    let attempts = boxes
        .cluster
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("patch warm-main-") && !call.contains("cold"))
        .count();
    assert_eq!(attempts, 2);
    assert_eq!(boxes.cluster.created().len(), 1);
    assert!(opened.box_id.as_str().starts_with("warm-main-cold-"));
}

#[tokio::test(start_paused = true)]
async fn warm_claim_an_error_of_the_api_server_that_is_not_a_conflict_is_a_port_fault() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes
        .cluster
        .fail_patches_touching(&PodKeys::default().claimed_at());

    let error = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("a fault");

    assert!(matches!(error, PortError::Fault(_)));
    assert_eq!(OpenFault::of(&error), None);
}

#[tokio::test(start_paused = true)]
async fn failing_pool_with_no_ready_warm_pod_open_returns_image_failing_at_once_and_makes_no_cold_pod()
 {
    let boxes = boxes();
    boxes.cluster.set_replica_set(Some(pool(vec![failing(
        "ImagePullFailing",
        "back-off pulling the image",
    )])));
    boxes.cluster.add_pod(not_ready("warm-main-a"));

    let error = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("a fault");

    assert_eq!(OpenFault::of(&error), Some(OpenFault::ImageFailing));
    assert!(matches!(error, PortError::Fault(_)));
    assert!(boxes.cluster.created().is_empty());
    assert!(boxes.cluster.deleted().is_empty());
}

#[tokio::test(start_paused = true)]
async fn failing_pool_with_a_ready_warm_pod_the_condition_does_not_stop_the_claim() {
    let boxes = boxes();
    boxes
        .cluster
        .set_replica_set(Some(pool(vec![failing("StartCrashing", "x")])));
    boxes.cluster.add_pod(warm_pod("warm-main-a"));

    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");

    assert_eq!(opened.box_id, BoxId::new("warm-main-a"));
    assert!(
        boxes
            .cluster
            .calls()
            .iter()
            .all(|call| !call.starts_with("replica_set"))
    );
}

#[tokio::test(start_paused = true)]
async fn failing_pool_a_condition_that_is_absent_or_false_is_read_as_not_true() {
    let boxes = boxes();
    let mut condition = failing("StartCrashing", "x");
    condition.status = "False".to_owned();
    boxes.cluster.set_replica_set(Some(pool(vec![condition])));

    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a cold box");

    assert!(opened.box_id.as_str().starts_with("warm-main-cold-"));
}

#[tokio::test(start_paused = true)]
async fn pool_a_pool_replica_set_that_does_not_exist_is_a_port_fault_and_not_an_image_error() {
    let boxes = boxes();
    boxes.cluster.set_replica_set(None);

    let error = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("a fault");

    assert!(matches!(error, PortError::Fault(_)));
    assert_eq!(OpenFault::of(&error), None);
    assert!(boxes.cluster.created().is_empty());
}

#[tokio::test(start_paused = true)]
async fn pool_an_api_server_that_cannot_be_reached_is_a_port_fault_for_every_call_of_open() {
    let boxes = boxes();
    boxes.cluster.fail_replica_sets();
    let error = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("a fault");
    assert!(matches!(error, PortError::Fault(_)));
    assert_eq!(OpenFault::of(&error), None);

    let lineage = Lineage::new();
    boxes.cluster.fail_lists();
    let error = boxes
        .sandbox
        .open(&fresh_request(lineage.after(crate::common::turn())))
        .await
        .expect_err("a fault");
    assert!(matches!(error, PortError::Fault(_)));
}

#[tokio::test(start_paused = true)]
async fn pool_a_failed_claim_never_deletes_the_pod_it_could_not_claim() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.rival_touches("warm-main-a");
    boxes.cluster.ready_on_create(true);

    boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a cold box");

    assert!(!boxes.cluster.deleted().contains(&"warm-main-a".to_owned()));
}
