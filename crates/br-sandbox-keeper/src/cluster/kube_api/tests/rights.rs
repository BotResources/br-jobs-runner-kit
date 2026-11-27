use std::collections::BTreeSet;

use http::StatusCode;
use serde_json::json;

use super::reads::{pod_json, pod_list, replica_set_json, watch_line};
use super::stub::{Seen, Wire};
use super::*;
use crate::cluster::{EventTarget, KubeApi, PoolApi, PoolEvent};
use crate::facts::PodVersion;
use crate::rights::{RIGHTS, RightScope};
use crate::rules::ConditionWrite;

const USED: [(&str, &str); 23] = [
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
    ("watch", "namespaces"),
    ("list", "validatingadmissionpolicybindings"),
    ("watch", "validatingadmissionpolicybindings"),
    ("list", "sandboxpools"),
    ("watch", "sandboxpools"),
    ("patch", "sandboxpools/status"),
    ("get", "configmaps"),
    ("get", "resourcequotas"),
    ("list", "networkpolicies"),
    ("create", "replicasets"),
    ("delete", "replicasets"),
];

fn right_of(seen: &Seen) -> (String, String) {
    let segments: Vec<&str> = seen.path.trim_start_matches('/').split('/').collect();
    let rest: &[&str] = match segments.as_slice() {
        ["api", "v1", rest @ ..] | ["apis", _, _, rest @ ..] => rest,
        _ => &[],
    };
    let (resource, named) = match rest {
        ["namespaces"] => ("namespaces".to_owned(), false),
        [resource] => ((*resource).to_owned(), false),
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
        ("GET", path) if seen.query.contains("watch=true") && !path.ends_with("/pods") => {
            return (StatusCode::OK, String::new());
        }
        ("GET", path) if path.ends_with("/validatingadmissionpolicybindings") => {
            json!({"apiVersion": "admissionregistration.k8s.io/v1",
                "kind": "ValidatingAdmissionPolicyBindingList", "metadata": {}, "items": []})
        }
        ("GET", path) if path.ends_with("/sandboxpools") => {
            json!({"apiVersion": "sandbox.botresources.ai/v1alpha1",
                "kind": "SandboxPoolList", "metadata": {}, "items": []})
        }
        ("GET", path) if path.ends_with("/networkpolicies") => {
            json!({"apiVersion": "networking.k8s.io/v1", "kind": "NetworkPolicyList",
                "metadata": {}, "items": []})
        }
        ("GET", path) if path.ends_with("/configmaps/br-sandbox-pod-template") => {
            json!({"apiVersion": "v1", "kind": "ConfigMap", "metadata": {"name": "t"}})
        }
        ("GET", path) if path.ends_with("/resourcequotas/br-sandbox-quota") => {
            json!({"apiVersion": "v1", "kind": "ResourceQuota", "metadata": {"name": "q"}})
        }
        ("POST", path) if path.ends_with("/replicasets") => replica_set_json(),
        (_, path) if path.contains("/sandboxpools/") => {
            json!({"apiVersion": "sandbox.botresources.ai/v1alpha1", "kind": "SandboxPool",
                "metadata": {"name": "main"}})
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
    let _ = cluster
        .watch_namespaces("br.sandbox/managed=true")
        .take(1)
        .collect::<Vec<_>>()
        .await;
    let _ = cluster.watch_bindings().take(1).collect::<Vec<_>>().await;
    let _ = cluster.watch_pools("sbx").take(1).collect::<Vec<_>>().await;
    cluster.read_template("sbx").await.unwrap();
    cluster.read_quota("sbx").await.unwrap();
    cluster.read_replica_set("sbx", "warm-sbx").await.unwrap();
    cluster.list_network_policies("sbx").await.unwrap();
    cluster
        .create_replica_set("sbx", &json!({"metadata": {"name": "warm-sbx"}}))
        .await
        .unwrap();
    cluster
        .apply_template("sbx", "warm-sbx", &json!({}))
        .await
        .unwrap();
    cluster
        .delete_replica_set("sbx", "warm-sbx", "rs-uid")
        .await
        .unwrap();
    cluster
        .patch_pool_status(
            "sbx",
            "main",
            &crate::resource::SandboxPoolStatus::default(),
        )
        .await
        .unwrap();
    let _ = cluster.watch_pods("sbx").take(2).collect::<Vec<_>>().await;
    cluster.get_pod("sbx", "w0").await.unwrap();
    cluster
        .delete_pod("sbx", "w0", &PodVersion::new("uid-w0", "7"))
        .await
        .unwrap();
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
    let granted: BTreeSet<(String, String)> = RIGHTS
        .iter()
        .flat_map(|right| {
            right
                .verbs
                .iter()
                .map(|verb| ((*verb).to_owned(), right.resource.to_owned()))
        })
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

fn scope_of(resource: &str) -> RightScope {
    RIGHTS
        .iter()
        .find(|right| right.resource == resource)
        .map(|right| right.scope)
        .expect("a granted resource")
}

#[tokio::test]
async fn every_call_is_made_at_the_scope_of_its_row_and_cluster_wide_only_where_the_table_says_so()
{
    let wire = Wire::routed(reply);
    drive_every_method(&wire).await;
    for seen in wire.all() {
        let (verb, resource) = right_of(&seen);
        let cluster_wide = !seen.path.contains("/namespaces/");
        let cluster_row = scope_of(&resource) == RightScope::Cluster;
        let leases = scope_of(&resource) == RightScope::LeaseNamespace;
        assert!(
            cluster_wide == cluster_row || (leases && !cluster_wide),
            "{verb} {resource} at {}",
            seen.path
        );
    }
}

#[tokio::test]
async fn the_template_config_map_is_read_by_the_one_granted_name_and_the_status_is_never_read() {
    let wire = Wire::routed(reply);
    drive_every_method(&wire).await;
    for seen in wire.all() {
        let (verb, resource) = right_of(&seen);
        if resource == "configmaps" {
            assert!(
                seen.path.ends_with("/configmaps/br-sandbox-pod-template"),
                "{}",
                seen.path
            );
        }
        if resource.ends_with("/status") {
            assert_eq!(verb, "patch", "{}", seen.path);
        }
        if resource == "pods" {
            assert!(seen.path.contains("/namespaces/sbx/pods"), "{}", seen.path);
        }
    }
}
