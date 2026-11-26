use br_agentic_harness_contract::{AgentId, ThreadRef};
use br_runner_sandbox::{ClusterError, PodKeys, Timestamp, cold_pod_manifest, cold_pod_marks};
use serde_json::json;
use uuid::Uuid;

use crate::common::config;

fn marks() -> br_runner_sandbox::ColdPodMarks {
    cold_pod_marks(
        &PodKeys::default(),
        config().pool_id(),
        ThreadRef::from_uuid(Uuid::now_v7()).expect("a thread"),
        AgentId::from_uuid(Uuid::now_v7()).expect("an agent"),
        Timestamp::parse("2026-11-26T10:00:00Z").expect("a time"),
    )
}

#[test]
fn cold_manifest_keeps_the_spec_and_the_role_label_of_the_template_byte_for_byte() {
    let template = json!({
        "metadata": {"labels": {"br.sandbox/role": "sandbox", "br.sandbox/pool": "warm"}},
        "spec": {"containers": [{"name": "sandbox", "image": "a:1"}], "hostUsers": false},
    });

    let manifest = cold_pod_manifest(&template, "warm-main-cold-", &marks()).expect("a manifest");

    assert_eq!(manifest.pointer("/spec"), template.pointer("/spec"));
    assert_eq!(
        manifest
            .pointer("/metadata/labels")
            .and_then(|labels| labels.get("br.sandbox/role")),
        Some(&json!("sandbox"))
    );
    assert_eq!(
        manifest
            .pointer("/metadata/labels")
            .and_then(|labels| labels.get("br.sandbox/pool")),
        Some(&json!("claimed"))
    );
}

#[test]
fn cold_manifest_takes_no_name_no_owner_and_no_other_metadata_from_the_template() {
    let template = json!({
        "metadata": {
            "name": "x",
            "ownerReferences": [{"kind": "ReplicaSet", "name": "warm-main"}],
            "finalizers": ["f"],
            "labels": {"a": "b"},
        },
        "spec": {"containers": []},
    });

    let manifest = cold_pod_manifest(&template, "warm-main-cold-", &marks()).expect("a manifest");

    assert_eq!(manifest.pointer("/metadata/name"), None);
    assert_eq!(manifest.pointer("/metadata/ownerReferences"), None);
    assert_eq!(manifest.pointer("/metadata/finalizers"), None);
    assert_eq!(
        manifest.pointer("/metadata/generateName"),
        Some(&json!("warm-main-cold-"))
    );
}

#[test]
fn cold_manifest_a_template_with_no_spec_is_an_error() {
    let error = cold_pod_manifest(&json!({"metadata": {}}), "x-", &marks()).expect_err("an error");

    assert!(matches!(error, ClusterError::Config(_)));
}
