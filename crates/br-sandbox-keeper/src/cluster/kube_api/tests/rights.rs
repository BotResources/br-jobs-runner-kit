use std::collections::BTreeSet;

use http::StatusCode;
use serde_json::json;

use super::reads::{pod_json, pod_list, replica_set_json, watch_line};
use super::stub::{Seen, Wire};
use super::*;
use crate::cluster::{EventTarget, KubeApi, PoolEvent};
use crate::facts::PodVersion;
use crate::rules::ConditionWrite;

const GRANTED: [(&str, &str); 16] = [
    ("get", "pods"),
    ("list", "pods"),
    ("watch", "pods"),
    ("delete", "pods"),
    ("get", "replicasets"),
    ("list", "replicasets"),
    ("watch", "replicasets"),
    ("patch", "replicasets"),
    ("patch", "replicasets/status"),
    ("create", "events"),
    ("get", "leases"),
    ("create", "leases"),
    ("update", "leases"),
    ("get", "namespaces"),
    ("list", "namespaces"),
    ("watch", "namespaces"),
];

const USED: [(&str, &str); 12] = [
    ("get", "pods"),
    ("list", "pods"),
    ("watch", "pods"),
    ("delete", "pods"),
    ("get", "replicasets"),
    ("patch", "replicasets"),
    ("patch", "replicasets/status"),
    ("create", "events"),
    ("get", "leases"),
    ("create", "leases"),
    ("update", "leases"),
    ("list", "namespaces"),
];

fn right_of(seen: &Seen) -> (String, String) {
    let segments: Vec<&str> = seen.path.trim_start_matches('/').split('/').collect();
    let rest: &[&str] = match segments.as_slice() {
        ["api", "v1", rest @ ..] | ["apis", _, _, rest @ ..] => rest,
        _ => &[],
    };
    let (resource, named) = match rest {
        ["namespaces"] => ("namespaces".to_owned(), false),
        ["namespaces", _, resource] => ((*resource).to_owned(), false),
        ["namespaces", _, resource, _] => ((*resource).to_owned(), true),
        ["namespaces", _, resource, _, sub] => (format!("{resource}/{sub}"), true),
        _ => ("unmapped".to_owned(), false),
    };
    let watching = seen.query.split('&').any(|part| part == "watch=true");
    let verb = match (seen.method.as_str(), named, watching) {
        ("GET", true, _) => "get",
        ("GET", false, true) => "watch",
        ("GET", false, false) => "list",
        ("POST", false, _) => "create",
        ("PUT", true, _) => "update",
        ("PATCH", true, _) => "patch",
        ("DELETE", true, _) => "delete",
        _ => "unmapped",
    };
    (verb.to_owned(), resource)
}

fn reply(seen: &Seen) -> (StatusCode, String) {
    let body = match (seen.method.as_str(), seen.path.as_str()) {
        ("GET", "/api/v1/namespaces") => {
            json!({"apiVersion": "v1", "kind": "NamespaceList", "metadata": {},
                "items": [{"metadata": {"name": "sbx"}}]})
        }
        ("GET", path) if path.ends_with("/pods") && seen.query.contains("watch=true") => {
            return (StatusCode::OK, watch_line());
        }
        ("GET", path) if path.ends_with("/pods") => pod_list(),
        (_, path) if path.contains("/pods/") => pod_json("w0", true),
        (_, path) if path.contains("/replicasets/") => replica_set_json(),
        (_, path) if path.contains("/leases") => json!({"apiVersion": "coordination.k8s.io/v1",
            "kind": "Lease", "metadata": {"name": "br-sandbox-keeper", "resourceVersion": "9"},
            "spec": {"holderIdentity": "keeper-a"}}),
        _ => json!({"apiVersion": "v1", "kind": "Event", "metadata": {"name": "e"},
            "involvedObject": {}}),
    };
    (StatusCode::OK, body.to_string())
}

async fn drive_every_method(wire: &Wire) {
    let cluster = wire.cluster();
    cluster
        .managed_namespaces("br.sandbox/managed=true")
        .await
        .unwrap();
    let _ = cluster.watch_pods("sbx").take(2).collect::<Vec<_>>().await;
    cluster.get_pod("sbx", "w0").await.unwrap();
    cluster
        .delete_pod("sbx", "w0", &PodVersion::new("uid-w0", "7"))
        .await
        .unwrap();
    cluster.get_replica_set("sbx", "warm-p").await.unwrap();
    cluster.apply_replicas("sbx", "warm-p", 4).await.unwrap();
    let write = ConditionWrite {
        status: "False",
        reason: "NoWarmPodFailing",
        message: String::new(),
        last_transition_time: at(1),
    };
    cluster
        .apply_condition("sbx", "warm-p", &write)
        .await
        .unwrap();
    let target = EventTarget::new("warm-p", "rs-uid");
    cluster
        .create_event("sbx", &target, PoolEvent::AtMax, "above the maximum")
        .await
        .unwrap();
    let lease = LeaseWrite::new(
        "keeper-a",
        at(1),
        at(2),
        std::time::Duration::from_secs(15),
        0,
    );
    cluster.get_lease("sys", "br-sandbox-keeper").await.unwrap();
    cluster
        .create_lease("sys", "br-sandbox-keeper", &lease)
        .await
        .unwrap();
    cluster
        .update_lease("sys", "br-sandbox-keeper", &lease, "9")
        .await
        .unwrap();
}

#[tokio::test]
async fn every_call_of_the_client_is_a_right_that_the_keeper_is_granted_and_nothing_else() {
    let wire = Wire::routed(reply);
    drive_every_method(&wire).await;
    let used: BTreeSet<(String, String)> = wire.all().iter().map(right_of).collect();
    let granted: BTreeSet<(String, String)> = GRANTED
        .iter()
        .map(|(verb, resource)| ((*verb).to_owned(), (*resource).to_owned()))
        .collect();
    assert!(used.is_subset(&granted), "{used:?}");
    let expected: BTreeSet<(String, String)> = USED
        .iter()
        .map(|(verb, resource)| ((*verb).to_owned(), (*resource).to_owned()))
        .collect();
    assert_eq!(used, expected);
    assert!(
        wire.all()
            .iter()
            .all(|seen| { seen.path.starts_with("/api/v1/") || seen.path.starts_with("/apis/") })
    );
}
