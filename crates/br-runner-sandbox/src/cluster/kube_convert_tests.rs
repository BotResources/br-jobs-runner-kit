use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Status;
use serde_json::{Value, json};

use super::error::ClusterError;
use super::kube_convert::{call_error, exit_code, pod_from_manifest, pod_view, replica_set_view};

fn status(value: Value) -> Status {
    serde_json::from_value(value).expect("a status")
}

fn pod(value: Value) -> Pod {
    serde_json::from_value(value).expect("a pod")
}

fn api_error(code: u16, reason: &str) -> kube::Error {
    kube::Error::Api(Box::new(
        kube::core::Status::failure("the api said no", reason).with_code(code),
    ))
}

#[test]
fn the_exit_code_of_a_success_status_is_zero() {
    assert_eq!(exit_code(&status(json!({"status": "Success"}))), Some(0));
}

#[test]
fn the_exit_code_of_a_non_zero_exit_is_the_code_of_its_exit_code_cause() {
    let failure = status(json!({
        "status": "Failure",
        "reason": "NonZeroExitCode",
        "message": "command terminated with non-zero exit code: exit status 3",
        "details": {"causes": [{"reason": "ExitCode", "message": "3"}]},
    }));
    assert_eq!(exit_code(&failure), Some(3));
}

#[test]
fn a_status_that_holds_no_exit_code_gives_no_exit_code() {
    let cases = [
        json!({}),
        json!({"status": "Failure", "reason": "InternalError"}),
        json!({"status": "Failure", "reason": "NonZeroExitCode"}),
        json!({"status": "Failure", "reason": "NonZeroExitCode", "details": {"causes": []}}),
        json!({
            "status": "Failure",
            "reason": "NonZeroExitCode",
            "details": {"causes": [{"reason": "ExitCode", "message": "many"}]},
        }),
        json!({
            "status": "Failure",
            "reason": "Other",
            "details": {"causes": [{"reason": "ExitCode", "message": "3"}]},
        }),
    ];
    for case in cases {
        assert_eq!(exit_code(&status(case.clone())), None, "{case}");
    }
}

#[test]
fn a_pod_view_reads_the_marks_and_the_readiness_of_a_pod() {
    let view = pod_view(&pod(json!({
        "metadata": {
            "name": "warm-main-x1",
            "resourceVersion": "4711",
            "labels": {"br.sandbox/pool": "warm"},
            "annotations": {"br.sandbox/clean": "true"},
        },
        "status": {
            "phase": "Running",
            "conditions": [
                {"type": "PodScheduled", "status": "True"},
                {"type": "Ready", "status": "True"},
            ],
        },
    })));
    assert_eq!(view.name, "warm-main-x1");
    assert_eq!(view.resource_version, "4711");
    assert_eq!(
        view.labels.get("br.sandbox/pool").map(String::as_str),
        Some("warm")
    );
    assert_eq!(
        view.annotations.get("br.sandbox/clean").map(String::as_str),
        Some("true")
    );
    assert!(view.is_ready());
}

#[test]
fn a_pod_view_is_not_ready_without_the_ready_condition_or_with_a_deletion_time() {
    let not_ready = pod_view(&pod(json!({
        "metadata": {"name": "a"},
        "status": {"phase": "Running", "conditions": [{"type": "Ready", "status": "False"}]},
    })));
    assert!(!not_ready.is_ready());
    let no_status = pod_view(&pod(json!({"metadata": {"name": "b"}})));
    assert!(!no_status.is_ready());
    let deleting = pod_view(&pod(json!({
        "metadata": {"name": "c", "deletionTimestamp": "2026-11-26T10:00:00Z"},
        "status": {"phase": "Running", "conditions": [{"type": "Ready", "status": "True"}]},
    })));
    assert!(deleting.deleting);
    assert!(!deleting.is_ready());
}

#[test]
fn a_replica_set_view_keeps_the_template_and_every_condition() {
    let replica_set: ReplicaSet = serde_json::from_value(json!({
        "metadata": {"name": "warm-main"},
        "spec": {
            "selector": {"matchLabels": {"a": "b"}},
            "template": {
                "metadata": {"labels": {"br.sandbox/pool": "warm"}},
                "spec": {"containers": [{"name": "sandbox", "image": "img"}]},
            },
        },
        "status": {
            "replicas": 2,
            "conditions": [{
                "type": "WarmPodsFailing",
                "status": "True",
                "reason": "StartCrashing",
                "message": "exit code 1",
                "lastTransitionTime": "2026-11-26T10:00:00Z",
            }],
        },
    }))
    .expect("a replica set");
    let view = replica_set_view(&replica_set).expect("a view");
    assert_eq!(view.name, "warm-main");
    assert_eq!(
        view.template["metadata"]["labels"]["br.sandbox/pool"],
        json!("warm")
    );
    assert_eq!(
        view.template["spec"]["containers"][0]["name"],
        json!("sandbox")
    );
    assert_eq!(view.conditions.len(), 1);
    assert_eq!(view.conditions[0].kind, "WarmPodsFailing");
    assert_eq!(view.conditions[0].reason.as_deref(), Some("StartCrashing"));
    assert!(view.warm_pods_failing().is_some());
}

#[test]
fn a_replica_set_with_no_status_has_no_condition() {
    let replica_set: ReplicaSet = serde_json::from_value(json!({
        "metadata": {"name": "warm-main"},
        "spec": {"selector": {}},
    }))
    .expect("a replica set");
    let view = replica_set_view(&replica_set).expect("a view");
    assert!(view.conditions.is_empty());
    assert!(view.warm_pods_failing().is_none());
}

#[test]
fn an_api_error_for_a_missing_pod_or_a_conflict_is_its_own_error_and_any_other_is_a_call_error() {
    assert_eq!(
        call_error("p1", api_error(404, "NotFound")),
        ClusterError::Gone("p1".to_owned())
    );
    assert_eq!(
        call_error("p1", api_error(409, "Conflict")),
        ClusterError::Conflict("p1".to_owned())
    );
    assert!(matches!(
        call_error("p1", api_error(500, "InternalError")),
        ClusterError::Call(_)
    ));
    assert!(matches!(
        call_error("p1", api_error(403, "Forbidden")),
        ClusterError::Call(_)
    ));
}

#[test]
fn a_pod_manifest_becomes_a_pod_that_keeps_its_marks_and_spec() {
    let manifest = json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "generateName": "warm-main-cold-",
            "labels": {"br.sandbox/pool": "claimed", "br.sandbox/role": "sandbox"},
            "annotations": {"br.sandbox/clean": "false"},
        },
        "spec": {"containers": [{"name": "sandbox", "image": "img"}]},
    });
    let pod = pod_from_manifest(&manifest).expect("a pod");
    assert_eq!(
        pod.metadata.generate_name.as_deref(),
        Some("warm-main-cold-")
    );
    assert!(pod.metadata.owner_references.is_none());
    let labels = pod.metadata.labels.expect("labels");
    assert_eq!(
        labels.get("br.sandbox/pool").map(String::as_str),
        Some("claimed")
    );
    let containers = pod.spec.expect("a spec").containers;
    assert_eq!(containers[0].name, "sandbox");
}

#[test]
fn a_manifest_that_is_not_a_pod_is_a_configuration_error() {
    let error = pod_from_manifest(&json!({"spec": {"containers": "no"}})).expect_err("an error");
    assert!(matches!(error, ClusterError::Config(_)));
}
