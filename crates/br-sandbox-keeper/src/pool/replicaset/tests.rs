use serde_json::json;

use super::*;

fn rendered() -> RenderedTemplate {
    RenderedTemplate {
        template: json!({"metadata": {"labels": {"br.sandbox/pool": "warm"}}, "spec": {"containers": []}}),
        hash: "h".to_owned(),
    }
}

#[test]
fn the_body_is_owned_by_the_resource_as_controller_with_no_block_owner_deletion_and_no_finalizer() {
    let body = replica_set_body(
        "sbx",
        &Owner::new("main", "uid-1"),
        3,
        &rendered(),
        &PodKeys::default(),
    );
    let reference = &body["metadata"]["ownerReferences"];
    assert_eq!(reference.as_array().unwrap().len(), 1);
    assert_eq!(
        reference[0],
        json!({"apiVersion": "sandbox.botresources.ai/v1alpha1", "kind": "SandboxPool",
            "name": "main", "uid": "uid-1", "controller": true})
    );
    assert!(reference[0].get("blockOwnerDeletion").is_none());
    assert!(body["metadata"].get("finalizers").is_none());
    assert_eq!(body["metadata"]["name"], "warm-sbx");
    assert_eq!(body["metadata"]["namespace"], "sbx");
}

#[test]
fn the_selector_is_the_two_labels_pool_warm_and_pool_id_and_the_size_is_the_given_one() {
    let body = replica_set_body(
        "sbx",
        &Owner::new("main", "uid-1"),
        5,
        &rendered(),
        &PodKeys::default(),
    );
    assert_eq!(
        body["spec"]["selector"],
        json!({"matchLabels": {"br.sandbox/pool": "warm", "br.sandbox/pool-id": "sbx"}})
    );
    assert_eq!(body["spec"]["replicas"], 5);
    assert_eq!(body["spec"]["template"], rendered().template);
}
