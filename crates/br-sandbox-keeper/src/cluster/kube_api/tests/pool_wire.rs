use http::StatusCode;
use serde_json::{Value, json};

use super::stub::Wire;
use super::*;
use crate::cluster::{PoolApi, ViewEvent};
use crate::resource::SandboxPoolStatus;

pub(super) fn status_reply(code: u16, reason: &str) -> Value {
    json!({"apiVersion": "v1", "kind": "Status", "status": "Failure", "message": reason,
        "reason": reason, "code": code})
}

pub(super) fn query_of(query: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

pub(super) fn listing(kind: &str, group_version: &str, items: Value) -> String {
    json!({"apiVersion": group_version, "kind": kind,
        "metadata": {"resourceVersion": "5"}, "items": items})
    .to_string()
}

pub(super) fn routed(kind: &'static str, group_version: &'static str, items: Value) -> Wire {
    Wire::routed(move |seen| {
        if seen.query.contains("watch=true") {
            (StatusCode::OK, String::new())
        } else {
            (StatusCode::OK, listing(kind, group_version, items.clone()))
        }
    })
}

#[tokio::test]
async fn the_namespaces_are_watched_cluster_wide_with_the_selector_as_label_selector() {
    let wire = routed(
        "NamespaceList",
        "v1",
        json!([{"metadata": {"name": "sbx",
        "labels": {"br.sandbox/managed": "true"}}}]),
    );
    let events: Vec<ViewEvent<_>> = wire
        .cluster()
        .watch_namespaces("br.sandbox/managed=true")
        .take(1)
        .collect()
        .await;
    assert!(matches!(&events[0], ViewEvent::Snapshot(found) if found.len() == 1));
    let seen = wire.all();
    assert_eq!(seen[0].path, "/api/v1/namespaces");
    assert!(query_of(&seen[0].query).contains(&(
        "labelSelector".to_owned(),
        "br.sandbox/managed=true".to_owned()
    )));
    assert_eq!(seen[1].path, "/api/v1/namespaces");
    assert!(seen[1].query.contains("watch=true"));
}

#[tokio::test]
async fn the_bindings_are_watched_cluster_wide_by_the_one_name() {
    let wire = routed(
        "ValidatingAdmissionPolicyBindingList",
        "admissionregistration.k8s.io/v1",
        json!([{"metadata": {"name": "br-sandbox-pod"}}]),
    );
    let events: Vec<ViewEvent<_>> = wire.cluster().watch_bindings().take(1).collect().await;
    assert!(matches!(&events[0], ViewEvent::Snapshot(found) if found.len() == 1));
    let seen = wire.all();
    assert_eq!(
        seen[0].path,
        "/apis/admissionregistration.k8s.io/v1/validatingadmissionpolicybindings"
    );
    assert!(query_of(&seen[0].query).contains(&(
        "fieldSelector".to_owned(),
        "metadata.name=br-sandbox-pod".to_owned()
    )));
}

#[tokio::test]
async fn the_pools_are_watched_in_one_namespace_and_a_bad_spec_does_not_fail_the_list() {
    let wire = routed(
        "SandboxPoolList",
        "sandbox.botresources.ai/v1alpha1",
        json!([{"metadata": {"name": "main", "uid": "u"}, "spec": {"unknown": [1, 2]}}]),
    );
    let events: Vec<ViewEvent<_>> = wire.cluster().watch_pools("sbx").take(1).collect().await;
    assert!(matches!(&events[0], ViewEvent::Snapshot(found) if found.len() == 1));
    let seen = wire.all();
    assert_eq!(
        seen[0].path,
        "/apis/sandbox.botresources.ai/v1alpha1/namespaces/sbx/sandboxpools"
    );
    assert_eq!(seen[0].method, "GET");
}

#[tokio::test(start_paused = true)]
async fn a_failed_list_of_the_pools_gives_a_failure_that_unsyncs_the_view() {
    let wire = Wire::routed(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            status_reply(500, "down").to_string(),
        )
    });
    let events: Vec<ViewEvent<_>> = wire.cluster().watch_pools("sbx").take(1).collect().await;
    assert!(matches!(&events[0], ViewEvent::Failed { reason }
        if reason.starts_with("the pool resource list failed")));
}

#[tokio::test]
async fn the_template_quota_and_policies_are_read_by_name_in_the_namespace() {
    let template = Wire::ok(json!({"apiVersion": "v1", "kind": "ConfigMap",
        "metadata": {"name": "br-sandbox-pod-template"}, "data": {"pod-template.json": "{}"}}));
    assert!(
        template
            .cluster()
            .read_template("sbx")
            .await
            .unwrap()
            .is_some()
    );
    let seen = template.only();
    assert_eq!(
        (seen.method.as_str(), seen.path.as_str()),
        (
            "GET",
            "/api/v1/namespaces/sbx/configmaps/br-sandbox-pod-template"
        )
    );
    let gone = Wire::answering(StatusCode::NOT_FOUND, status_reply(404, "NotFound"));
    assert_eq!(gone.cluster().read_template("sbx").await, Ok(None));
    let no_quota = Wire::answering(StatusCode::NOT_FOUND, status_reply(404, "NotFound"));
    assert_eq!(no_quota.cluster().read_quota("sbx").await, Ok(None));
    assert_eq!(
        no_quota.only().path,
        "/api/v1/namespaces/sbx/resourcequotas/br-sandbox-quota"
    );
    let policies = routed(
        "NetworkPolicyList",
        "networking.k8s.io/v1",
        json!([{"metadata": {"name": "br-sandbox-default-deny"}}]),
    );
    let found = policies
        .cluster()
        .list_network_policies("sbx")
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        policies.only().path,
        "/apis/networking.k8s.io/v1/namespaces/sbx/networkpolicies"
    );
}

#[tokio::test]
async fn a_failed_read_is_an_error_and_never_an_empty_answer() {
    let broken = Wire::answering(StatusCode::INTERNAL_SERVER_ERROR, status_reply(500, "Oops"));
    assert!(matches!(
        broken.cluster().read_quota("sbx").await,
        Err(ApiError::Call(_))
    ));
    assert!(matches!(
        broken.cluster().list_network_policies("sbx").await,
        Err(ApiError::Call(_))
    ));
    assert!(matches!(
        broken.cluster().read_replica_set("sbx", "warm-sbx").await,
        Err(ApiError::Call(_))
    ));
}

#[tokio::test]
async fn a_refusal_of_the_api_server_is_a_typed_error() {
    let wire = Wire::answering(StatusCode::FORBIDDEN, status_reply(403, "Forbidden"));
    assert!(matches!(
        wire.cluster().read_quota("sbx").await,
        Err(ApiError::Forbidden(_))
    ));
    assert!(matches!(
        wire.cluster()
            .delete_replica_set("sbx", "warm-sbx", "u")
            .await,
        Err(ApiError::Forbidden(_))
    ));
    let status = SandboxPoolStatus::default();
    assert!(matches!(
        wire.cluster()
            .patch_pool_status("sbx", "main", &status)
            .await,
        Err(ApiError::Forbidden(_))
    ));
}

#[tokio::test(start_paused = true)]
async fn a_pool_list_that_the_api_server_does_not_serve_is_a_missing_definition_and_other_failures_are_not()
 {
    let absent = Wire::answering(StatusCode::NOT_FOUND, status_reply(404, "NotFound"));
    let events: Vec<ViewEvent<_>> = absent.cluster().watch_pools("sbx").take(1).collect().await;
    assert!(matches!(&events[0], ViewEvent::Missing { reason }
        if reason.contains("pool resource list failed") && reason.contains("does not serve")));
    let broken = Wire::answering(
        StatusCode::INTERNAL_SERVER_ERROR,
        status_reply(500, "InternalError"),
    );
    let events: Vec<ViewEvent<_>> = broken.cluster().watch_pools("sbx").take(1).collect().await;
    assert!(matches!(&events[0], ViewEvent::Failed { .. }));
}
