use http::StatusCode;
use serde_json::{Value, json};

use super::stub::Wire;
use super::*;
use crate::cluster::KubeApi;
use crate::facts::PoolState;

pub(super) fn pod_json(name: &str, labelled: bool) -> Value {
    let labels = if labelled {
        json!({"br.sandbox/pool": "warm", "br.sandbox/pool-id": "p"})
    } else {
        json!({"other": "label"})
    };
    json!({"apiVersion": "v1", "kind": "Pod", "metadata": {
        "name": name, "namespace": "sbx", "uid": format!("uid-{name}"),
        "resourceVersion": "7", "labels": labels,
        "creationTimestamp": "2026-01-01T00:00:00Z"}})
}

pub(super) fn pod_list() -> Value {
    json!({"apiVersion": "v1", "kind": "PodList", "metadata": {"resourceVersion": "100"},
        "items": [pod_json("w0", true), pod_json("other", false)]})
}

pub(super) fn watch_line() -> String {
    format!(
        "{}\n",
        json!({"type": "ADDED", "object": pod_json("w1", true)})
    )
}

pub(super) fn replica_set_json() -> Value {
    json!({"apiVersion": "apps/v1", "kind": "ReplicaSet",
        "metadata": {"name": "warm-p", "uid": "rs-uid"}, "spec": {"replicas": 3}})
}

fn not_found() -> Value {
    json!({"apiVersion": "v1", "kind": "Status", "status": "Failure", "message": "gone",
        "reason": "NotFound", "code": 404})
}

#[tokio::test]
async fn a_pod_is_read_by_name_and_a_pod_without_the_pool_label_is_not_ours() {
    let wire = Wire::ok(pod_json("w0", true));
    let found = wire.cluster().get_pod("sbx", "w0").await.unwrap().unwrap();
    assert_eq!(
        (found.name.as_str(), found.pool_state),
        ("w0", PoolState::Warm)
    );
    let seen = wire.only();
    assert_eq!(seen.method, "GET");
    assert_eq!(seen.path, "/api/v1/namespaces/sbx/pods/w0");
    let foreign = Wire::ok(pod_json("w0", false));
    assert_eq!(foreign.cluster().get_pod("sbx", "w0").await, Ok(None));
    let gone = Wire::answering(StatusCode::NOT_FOUND, not_found());
    assert_eq!(gone.cluster().get_pod("sbx", "w0").await, Ok(None));
}

#[tokio::test]
async fn a_replica_set_is_read_by_name() {
    let wire = Wire::ok(replica_set_json());
    let found = wire
        .cluster()
        .get_replica_set("sbx", "warm-p")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((found.uid.as_str(), found.replicas), ("rs-uid", Some(3)));
    let seen = wire.only();
    assert_eq!(seen.method, "GET");
    assert_eq!(seen.path, "/apis/apps/v1/namespaces/sbx/replicasets/warm-p");
    let gone = Wire::answering(StatusCode::NOT_FOUND, not_found());
    assert_eq!(
        gone.cluster().get_replica_set("sbx", "warm-p").await,
        Ok(None)
    );
}

#[tokio::test]
async fn the_pods_are_listed_then_watched_from_the_version_of_the_list_with_the_pool_selector() {
    let wire = Wire::routed(|seen| {
        if seen.query.contains("watch=true") {
            (StatusCode::OK, watch_line())
        } else {
            (StatusCode::OK, pod_list().to_string())
        }
    });
    let events: Vec<PodEvent> = wire.cluster().watch_pods("sbx").take(2).collect().await;
    match &events[0] {
        PodEvent::Snapshot(pods) => {
            assert_eq!(pods.len(), 1);
            assert_eq!(pods[0].name, "w0");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&events[1], PodEvent::Applied(pod) if pod.name == "w1"));
    let seen = wire.all();
    let selector = (
        "labelSelector".to_owned(),
        "br.sandbox/pool in (warm,claimed)".to_owned(),
    );
    let query_of = |query: &str| -> Vec<(String, String)> {
        url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect()
    };
    assert_eq!(seen[0].method, "GET");
    assert_eq!(seen[0].path, "/api/v1/namespaces/sbx/pods");
    assert!(query_of(&seen[0].query).contains(&selector));
    assert!(!seen[0].query.contains("watch=true"));
    assert_eq!(seen[1].path, "/api/v1/namespaces/sbx/pods");
    let watched = query_of(&seen[1].query);
    assert!(watched.contains(&selector));
    assert!(watched.contains(&("watch".to_owned(), "true".to_owned())));
    assert!(watched.contains(&("resourceVersion".to_owned(), "100".to_owned())));
}

fn lease_write() -> LeaseWrite {
    LeaseWrite::new(
        "keeper-a",
        at(100),
        at(200),
        std::time::Duration::from_secs(15),
        3,
    )
}

fn lease_json() -> Value {
    serde_json::to_value(lease_object(
        "br-sandbox-keeper",
        "sys",
        &lease_write(),
        Some("42"),
    ))
    .unwrap()
}

#[tokio::test]
async fn the_lease_is_read_created_and_replaced_by_its_name_in_its_namespace() {
    let read = Wire::ok(lease_json());
    let facts = read
        .cluster()
        .get_lease("sys", "br-sandbox-keeper")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(facts.holder.as_deref(), Some("keeper-a"));
    let seen = read.only();
    assert_eq!(seen.method, "GET");
    assert_eq!(
        seen.path,
        "/apis/coordination.k8s.io/v1/namespaces/sys/leases/br-sandbox-keeper"
    );
    let gone = Wire::answering(StatusCode::NOT_FOUND, not_found());
    assert_eq!(
        gone.cluster().get_lease("sys", "br-sandbox-keeper").await,
        Ok(None)
    );

    let create = Wire::ok(lease_json());
    create
        .cluster()
        .create_lease("sys", "br-sandbox-keeper", &lease_write())
        .await
        .unwrap();
    let seen = create.only();
    assert_eq!(seen.method, "POST");
    assert_eq!(
        seen.path,
        "/apis/coordination.k8s.io/v1/namespaces/sys/leases"
    );
    assert_eq!(seen.body["spec"]["holderIdentity"], "keeper-a");
    assert_eq!(seen.body["metadata"]["name"], "br-sandbox-keeper");

    let replace = Wire::ok(lease_json());
    replace
        .cluster()
        .update_lease("sys", "br-sandbox-keeper", &lease_write(), "42")
        .await
        .unwrap();
    let seen = replace.only();
    assert_eq!(seen.method, "PUT");
    assert_eq!(
        seen.path,
        "/apis/coordination.k8s.io/v1/namespaces/sys/leases/br-sandbox-keeper"
    );
    assert_eq!(seen.body["metadata"]["resourceVersion"], "42");
    assert_eq!(seen.body["spec"]["leaseDurationSeconds"], 15);
}
