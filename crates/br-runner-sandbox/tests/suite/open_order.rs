use br_agentic_harness_contract::{
    BoxId, FreshReason, PortError, SandboxPort, StartKind, WebAccess,
};
use br_runner_sandbox::PodKeys;
use serde_json::json;

use crate::fakes::{Lineage, boxes, fresh_request, warm_pod};

fn fault_text(error: &PortError) -> String {
    format!("{error:?}")
}

#[tokio::test(start_paused = true)]
async fn mode_check_a_web_access_that_does_not_match_the_mode_is_a_refusal_made_before_any_call_to_the_cluster_or_the_store()
 {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let lineage = Lineage::new();
    let mut open = fresh_request(lineage.after(crate::common::turn()));
    open.web_access = Some(WebAccess::Full);

    let error = boxes.sandbox.open(&open).await.expect_err("refused");

    assert!(error.is_refusal(), "{}", fault_text(&error));
    assert!(boxes.cluster.calls().is_empty());
    assert!(boxes.store.log().is_empty());
    assert!(boxes.cluster.scripts.execs().is_empty());
}

#[tokio::test(start_paused = true)]
async fn mode_check_a_refused_open_does_not_keep_the_instance() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let lineage = Lineage::new();
    let mut open = fresh_request(lineage.first());
    open.web_access = Some(WebAccess::Full);
    boxes.sandbox.open(&open).await.expect_err("refused");

    open.web_access = None;
    let opened = boxes.sandbox.open(&open).await.expect("an open box");

    assert_eq!(opened.box_id, BoxId::new("warm-main-a"));
}

#[tokio::test(start_paused = true)]
async fn instance_an_instance_with_an_open_box_is_refused_until_the_box_is_released() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.add_pod(warm_pod("warm-main-b"));
    let lineage = Lineage::new();
    let open = fresh_request(lineage.first());
    let first = boxes.sandbox.open(&open).await.expect("a box");

    let second = boxes.sandbox.open(&open).await.expect_err("refused");
    boxes
        .sandbox
        .release(&first.box_id)
        .await
        .expect("released");
    let third = boxes.sandbox.open(&open).await;

    assert!(second.is_refusal());
    assert!(third.is_ok());
}

#[tokio::test(start_paused = true)]
async fn instance_two_instances_of_two_agents_have_two_pods_and_two_bindings() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    boxes.cluster.add_pod(warm_pod("warm-main-b"));
    let (one, other) = (Lineage::new(), Lineage::new());

    let first = boxes
        .sandbox
        .open(&fresh_request(one.first()))
        .await
        .expect("first");
    let second = boxes
        .sandbox
        .open(&fresh_request(other.first()))
        .await
        .expect("second");

    assert_ne!(first.box_id, second.box_id);
    let keys = PodKeys::default();
    let labelled = |name: &str| {
        boxes
            .cluster
            .pod(name)
            .expect("a pod")
            .labels
            .get(&keys.thread())
            .cloned()
    };
    assert_eq!(
        labelled(first.box_id.as_str()),
        Some(one.thread.to_string())
    );
    assert_eq!(
        labelled(second.box_id.as_str()),
        Some(other.thread.to_string())
    );
}

#[tokio::test(start_paused = true)]
async fn no_parent_a_request_with_no_parent_is_fresh_with_no_save_ever_and_no_call_to_the_store() {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let lineage = Lineage::new();

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.first()))
        .await
        .expect("a box");

    assert_eq!(opened.start, StartKind::Fresh(FreshReason::NoSaveEver));
    assert!(boxes.store.log().is_empty());
}

#[tokio::test(start_paused = true)]
async fn warm_claim_the_claim_is_one_patch_with_the_version_read_that_binds_the_thread_and_the_agent()
 {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let before = boxes.cluster.pod("warm-main-a").expect("a pod");
    let lineage = Lineage::new();

    let opened = boxes
        .sandbox
        .open(&fresh_request(lineage.first()))
        .await
        .expect("a box");

    let keys = PodKeys::default();
    let claims: Vec<_> = boxes
        .cluster
        .patches()
        .into_iter()
        .filter(|(_, patch)| patch.pointer("/metadata/resourceVersion").is_some())
        .collect();
    assert_eq!(claims.len(), 1);
    let (name, patch) = &claims[0];
    assert_eq!(name, opened.box_id.as_str());
    assert_eq!(
        patch.pointer("/metadata/resourceVersion"),
        Some(&json!(before.resource_version))
    );
    let after = boxes.cluster.pod(name).expect("a pod");
    assert_eq!(
        after.labels.get(&keys.pool()).map(String::as_str),
        Some("claimed")
    );
    assert_eq!(
        after.labels.get(&keys.thread()),
        Some(&lineage.thread.to_string())
    );
    assert_eq!(
        after.labels.get(&keys.agent()),
        Some(&lineage.agent.to_string())
    );
    assert_eq!(
        after.annotations.get(&keys.clean()).map(String::as_str),
        Some("false")
    );
    assert!(after.annotations.contains_key(&keys.claimed_at()));
}

#[tokio::test(start_paused = true)]
async fn warm_claim_no_patch_changes_the_role_label_the_pool_id_label_or_puts_a_pod_back_in_the_pool()
 {
    let boxes = boxes();
    boxes.cluster.add_pod(warm_pod("warm-main-a"));
    let before = boxes.cluster.pod("warm-main-a").expect("a pod");
    let lineage = Lineage::new();
    boxes
        .sandbox
        .open(&fresh_request(lineage.first()))
        .await
        .expect("a box");

    let keys = PodKeys::default();
    let after = boxes.cluster.pod("warm-main-a").expect("a pod");
    assert_eq!(
        after.labels.get(&keys.role()),
        before.labels.get(&keys.role())
    );
    assert_eq!(
        after.labels.get(&keys.pool_id()),
        before.labels.get(&keys.pool_id())
    );
    for (_, patch) in boxes.cluster.patches() {
        assert_ne!(
            patch.pointer(&format!(
                "/metadata/labels/{}",
                keys.pool().replace('/', "~1")
            )),
            Some(&json!("warm"))
        );
        assert!(
            patch
                .pointer(&format!(
                    "/metadata/labels/{}",
                    keys.role().replace('/', "~1")
                ))
                .is_none()
        );
        assert!(
            patch
                .pointer(&format!(
                    "/metadata/labels/{}",
                    keys.pool_id().replace('/', "~1")
                ))
                .is_none()
        );
        assert!(patch.get("spec").is_none());
    }
}
