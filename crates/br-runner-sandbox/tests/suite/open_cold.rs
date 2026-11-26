use std::time::Duration;

use br_agentic_harness_contract::{
    BoxId, FreshReason, OpenFault, PortError, SandboxPort, StartKind,
};
use br_runner_sandbox::PodKeys;
use serde_json::{Value, json};

use crate::fakes::{
    Lineage, Script, boxes, boxes_with, failing, fresh_request, pool, pool_template, staged_save,
};

fn cold_name() -> &'static str {
    "warm-main-cold-1"
}

#[tokio::test(start_paused = true)]
async fn cold_pod_is_made_from_the_pool_template_with_the_claim_labels_and_annotations_and_no_owner()
 {
    let boxes = boxes();
    let lineage = Lineage::new();

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.first()))
        .await
        .expect("a box");

    assert_eq!(opened.box_id, BoxId::new(cold_name()));
    assert_eq!(opened.start, StartKind::Fresh(FreshReason::NoSaveEver));
    let manifest = boxes.cluster.created().remove(0);
    let keys = PodKeys::default();
    let template = pool_template();
    assert_eq!(manifest.pointer("/spec"), template.pointer("/spec"));
    assert!(manifest.pointer("/metadata/ownerReferences").is_none());
    let label = |key: String| {
        manifest
            .pointer(&format!("/metadata/labels/{}", key.replace('/', "~1")))
            .cloned()
    };
    let note = |key: String| {
        manifest
            .pointer(&format!("/metadata/annotations/{}", key.replace('/', "~1")))
            .cloned()
    };
    assert_eq!(label(keys.role()), Some(json!("sandbox")));
    assert_eq!(label(keys.pool_id()), Some(json!("main")));
    assert_eq!(label(keys.pool()), Some(json!("claimed")));
    assert_eq!(
        label(keys.thread()),
        Some(json!(lineage.thread.to_string()))
    );
    assert_eq!(label(keys.agent()), Some(json!(lineage.agent.to_string())));
    assert_eq!(label("app".to_owned()), Some(json!("sandbox")));
    assert_eq!(note(keys.clean()), Some(json!("false")));
    assert!(note(keys.claimed_at()).is_some());
    assert_eq!(note("example/note".to_owned()), Some(json!("kept")));
}

#[tokio::test(start_paused = true)]
async fn cold_pod_no_patch_changes_the_role_label_or_the_spec() {
    let boxes = boxes();
    boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");

    let keys = PodKeys::default();
    for (_, patch) in boxes.cluster.patches() {
        assert!(patch.get("spec").is_none());
        assert!(
            patch
                .pointer("/metadata/labels")
                .is_none_or(|labels: &Value| labels.get(keys.role()).is_none())
        );
    }
    let pod = boxes.cluster.pod(cold_name()).expect("a pod");
    assert_eq!(
        pod.labels.get(&keys.role()).map(String::as_str),
        Some("sandbox")
    );
}

#[tokio::test(start_paused = true)]
async fn cold_pod_the_adapter_waits_until_the_pod_is_ready_within_the_open_timeout() {
    let boxes = boxes();
    boxes.cluster.ready_on_create(false);
    let cluster = boxes.cluster.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        cluster.make_ready(cold_name());
    });

    let opened = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect("a box");

    assert_eq!(opened.box_id, BoxId::new(cold_name()));
    assert!(boxes.cluster.deleted().is_empty());
}

#[tokio::test(start_paused = true)]
async fn cold_pod_that_is_not_ready_after_the_open_timeout_is_deleted_and_the_error_is_image_unavailable()
 {
    let boxes = boxes_with(json!({"open_timeout_seconds": 10}));
    boxes.cluster.ready_on_create(false);

    let error = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("a fault");

    assert_eq!(OpenFault::of(&error), Some(OpenFault::ImageUnavailable));
    assert!(matches!(error, PortError::Fault(_)));
    assert_eq!(boxes.cluster.deleted(), vec![cold_name().to_owned()]);
}

#[tokio::test(start_paused = true)]
async fn cold_pod_that_is_not_ready_when_the_pool_condition_is_true_gives_image_failing_after_the_cold_pod_is_deleted()
 {
    let boxes = boxes_with(json!({"open_timeout_seconds": 10}));
    boxes.cluster.ready_on_create(false);
    boxes.cluster.add_pod({
        let mut pod = crate::fakes::warm_pod("warm-main-a");
        pod.ready_condition = false;
        pod
    });
    boxes.cluster.set_replica_set(Some(pool(Vec::new())));

    let cluster = boxes.cluster.clone();
    let judged = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        cluster.set_replica_set(Some(pool(vec![failing("StartCrashing", "exit 1")])));
    });
    let error = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await;
    judged.await.expect("the task ended");

    let error = match error {
        Err(error) => error,
        Ok(_) => panic!("the open must fail"),
    };
    assert_eq!(OpenFault::of(&error), Some(OpenFault::ImageFailing));
    let calls = boxes.cluster.calls();
    let deleted = calls
        .iter()
        .position(|call| call == &format!("delete {}", cold_name()))
        .expect("deleted");
    let judged = calls
        .iter()
        .rposition(|call| call.starts_with("replica_set"))
        .expect("read");
    assert!(deleted < judged);
}

#[tokio::test(start_paused = true)]
async fn cold_create_false_with_no_ready_warm_pod_ends_with_image_unavailable_and_makes_no_pod() {
    let boxes = boxes_with(json!({"cold_create": false}));

    let error = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("a fault");

    assert_eq!(OpenFault::of(&error), Some(OpenFault::ImageUnavailable));
    assert!(boxes.cluster.created().is_empty());
}

#[tokio::test(start_paused = true)]
async fn cold_pod_the_restore_and_the_seed_run_on_the_cold_pod_like_on_a_warm_pod() {
    let boxes = boxes();
    let lineage = Lineage::new();
    let parent = crate::common::turn();
    staged_save(&boxes, &lineage, parent);
    boxes
        .cluster
        .scripts
        .push_script(Script::like_bsdtar_extract());

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.after(parent)))
        .await
        .expect("a box");

    assert_eq!(opened.start, StartKind::Restored { turn: parent });
    assert_eq!(opened.box_id, BoxId::new(cold_name()));
    assert_eq!(boxes.cluster.scripts.execs()[0].request.pod, cold_name());
}

#[tokio::test(start_paused = true)]
async fn cold_pod_when_the_seed_write_fails_on_it_the_cold_pod_is_deleted() {
    let boxes = boxes();
    boxes
        .cluster
        .fail_patches_touching(&PodKeys::default().seed_hash());

    let error = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("a fault");

    assert!(matches!(error, PortError::Fault(_)));
    assert_eq!(boxes.cluster.deleted(), vec![cold_name().to_owned()]);
}
