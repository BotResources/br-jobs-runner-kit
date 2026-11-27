use http::StatusCode;
use serde_json::{Value, json};

use super::pool_wire::status_reply;
use super::stub::Wire;
use super::*;
use crate::cluster::{CreateOutcome, PoolApi};
use crate::resource::SandboxPoolStatus;

#[tokio::test]
async fn the_replica_set_is_created_by_a_post_that_keeps_every_field_of_the_template() {
    let body = json!({"apiVersion": "apps/v1", "kind": "ReplicaSet",
        "metadata": {"name": "warm-sbx", "namespace": "sbx"},
        "spec": {"replicas": 2, "template": {"spec": {"futureSecurityField": {"on": true}}}}});
    let wire = Wire::ok(json!({"apiVersion": "apps/v1", "kind": "ReplicaSet",
        "metadata": {"name": "warm-sbx"}}));
    let outcome = wire.cluster().create_replica_set("sbx", &body).await;
    assert_eq!(outcome, Ok(CreateOutcome::Created));
    let seen = wire.only();
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.path, "/apis/apps/v1/namespaces/sbx/replicasets");
    assert!(seen.query.contains("fieldManager=br-sandbox-keeper"));
    assert_eq!(seen.body, body);
}

#[tokio::test]
async fn a_create_that_answers_already_exists_is_exists_and_other_conflicts_are_errors() {
    let taken = json!({"apiVersion": "v1", "kind": "Status", "status": "Failure",
        "message": "exists", "reason": "AlreadyExists", "code": 409});
    let wire = Wire::answering(StatusCode::CONFLICT, taken);
    let outcome = wire.cluster().create_replica_set("sbx", &json!({})).await;
    assert_eq!(outcome, Ok(CreateOutcome::Exists));
    let other = Wire::answering(StatusCode::CONFLICT, status_reply(409, "Conflict"));
    assert_eq!(
        other.cluster().create_replica_set("sbx", &json!({})).await,
        Err(ApiError::Conflict)
    );
}

#[tokio::test]
async fn the_template_is_written_by_a_server_side_apply_of_spec_template_alone() {
    let template = json!({"metadata": {"annotations": {"br.sandbox/template-hash": "h"}}});
    let wire = Wire::ok(json!({"apiVersion": "apps/v1", "kind": "ReplicaSet",
        "metadata": {"name": "warm-sbx"}}));
    wire.cluster()
        .apply_template("sbx", "warm-sbx", &template)
        .await
        .unwrap();
    let seen = wire.only();
    assert_eq!(seen.method, "PATCH");
    assert_eq!(
        seen.path,
        "/apis/apps/v1/namespaces/sbx/replicasets/warm-sbx"
    );
    assert!(seen.query.contains("fieldManager=br-sandbox-keeper"));
    assert_eq!(
        seen.content_type.as_deref(),
        Some("application/apply-patch+yaml")
    );
    assert_eq!(
        seen.body,
        json!({"apiVersion": "apps/v1", "kind": "ReplicaSet",
            "metadata": {"name": "warm-sbx"}, "spec": {"template": template}})
    );
}

#[tokio::test]
async fn the_replica_set_is_deleted_with_the_uid_precondition_and_background_propagation() {
    let wire = Wire::ok(json!({"apiVersion": "v1", "kind": "Status", "status": "Success"}));
    let outcome = wire
        .cluster()
        .delete_replica_set("sbx", "warm-sbx", "rs-uid")
        .await;
    assert_eq!(outcome, Ok(DeleteOutcome::Deleted));
    let seen = wire.only();
    assert_eq!(seen.method, "DELETE");
    assert_eq!(
        seen.path,
        "/apis/apps/v1/namespaces/sbx/replicasets/warm-sbx"
    );
    assert_eq!(seen.body["preconditions"], json!({"uid": "rs-uid"}));
    assert_eq!(seen.body["propagationPolicy"], "Background");
    let conflict = Wire::answering(StatusCode::CONFLICT, status_reply(409, "Conflict"));
    assert_eq!(
        conflict
            .cluster()
            .delete_replica_set("sbx", "warm-sbx", "x")
            .await,
        Ok(DeleteOutcome::Conflict)
    );
    let gone = Wire::answering(StatusCode::NOT_FOUND, status_reply(404, "NotFound"));
    assert_eq!(
        gone.cluster()
            .delete_replica_set("sbx", "warm-sbx", "x")
            .await,
        Ok(DeleteOutcome::Gone)
    );
}

#[tokio::test]
async fn the_status_is_patched_on_the_status_subresource_and_an_absent_field_is_cleared() {
    let wire = Wire::ok(json!({"apiVersion": "sandbox.botresources.ai/v1alpha1",
        "kind": "SandboxPool", "metadata": {"name": "main"}}));
    let status = SandboxPoolStatus {
        desired_replicas: Some(3),
        ..SandboxPoolStatus::default()
    };
    wire.cluster()
        .patch_pool_status("sbx", "main", &status)
        .await
        .unwrap();
    let seen = wire.only();
    assert_eq!(seen.method, "PATCH");
    assert_eq!(
        seen.path,
        "/apis/sandbox.botresources.ai/v1alpha1/namespaces/sbx/sandboxpools/main/status"
    );
    assert_eq!(
        seen.content_type.as_deref(),
        Some("application/merge-patch+json")
    );
    let written = seen.body["status"].as_object().unwrap();
    assert_eq!(written["desiredReplicas"], 3);
    for cleared in [
        "observedGeneration",
        "conditions",
        "replicas",
        "readyReplicas",
        "updatedReplicas",
        "observedAt",
    ] {
        assert_eq!(written[cleared], Value::Null, "{cleared}");
    }
    assert!(seen.body.get("spec").is_none());
}
