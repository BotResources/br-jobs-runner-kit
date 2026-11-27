use serde_json::json;

use super::*;

fn pod(value: serde_json::Value) -> Pod {
    serde_json::from_value(value).unwrap()
}

fn claimed_pod() -> Pod {
    pod(json!({
        "metadata": {
            "name": "p1", "namespace": "sbx", "uid": "u1", "resourceVersion": "7",
            "creationTimestamp": "2026-01-01T00:00:10Z",
            "labels": {
                "br.sandbox/pool": "claimed", "br.sandbox/pool-id": "acme",
                "br.sandbox/thread": "t", "br.sandbox/agent": "a"
            },
            "annotations": {
                "br.sandbox/clean": "true",
                "br.sandbox/claimed-at": "2026-01-01T00:01:00Z",
                "br.sandbox/idle-since": "2026-01-01T00:02:00.500Z",
                "br.sandbox/state-bytes": "12345"
            }
        },
        "status": {
            "phase": "Running",
            "conditions": [{"type": "Ready", "status": "True"}],
            "containerStatuses": [{
                "name": "c", "image": "i", "imageID": "", "ready": true, "restartCount": 0,
                "state": {"waiting": {"reason": "CrashLoopBackOff"}}
            }],
            "initContainerStatuses": [{
                "name": "init", "image": "i", "imageID": "", "ready": false, "restartCount": 0,
                "state": {"terminated": {"exitCode": 3, "reason": "Error"}}
            }]
        }
    }))
}

#[test]
fn a_pod_without_the_pool_state_label_is_not_ours() {
    let keys = PodKeys::default();
    let bare = pod(json!({"metadata": {"name": "proxy", "labels": {"br.sandbox/role": "proxy"}}}));
    assert!(pod_facts(&bare, &keys).is_none());
    let other = pod(json!({"metadata": {"name": "x", "labels": {"br.sandbox/pool": "probe"}}}));
    assert!(pod_facts(&other, &keys).is_none());
}

#[test]
fn a_claimed_pod_is_read_from_its_labels_and_annotations() {
    let facts = pod_facts(&claimed_pod(), &PodKeys::default()).unwrap();
    assert_eq!(facts.pool_state, PoolState::Claimed);
    assert_eq!(facts.pool_id.as_deref(), Some("acme"));
    assert_eq!(facts.thread.as_deref(), Some("t"));
    assert_eq!(facts.clean, Mark::Value(true));
    assert_eq!(
        facts.claimed_at,
        Mark::Value(Timestamp::parse("2026-01-01T00:01:00Z").unwrap())
    );
    assert_eq!(
        facts.idle_since,
        Mark::Value(Timestamp::parse("2026-01-01T00:02:00.500Z").unwrap())
    );
    assert_eq!(facts.state_bytes, Mark::Value(12345));
    assert_eq!(
        facts.created_at,
        Timestamp::parse("2026-01-01T00:00:10Z").unwrap()
    );
    assert!(facts.ready);
    assert!(!facts.owned);
    assert_eq!(facts.phase, Phase::Running);
    assert_eq!(facts.version().resource_version, "7");
}

#[test]
fn the_container_states_of_init_and_main_containers_are_read() {
    let facts = pod_facts(&claimed_pod(), &PodKeys::default()).unwrap();
    assert_eq!(facts.containers.len(), 2);
    assert_eq!(
        facts.containers[0].terminated.as_ref().unwrap().exit_code,
        3
    );
    assert_eq!(
        facts.containers[1].waiting_reason.as_deref(),
        Some("CrashLoopBackOff")
    );
}

#[test]
fn an_annotation_that_cannot_be_read_is_unreadable_and_a_missing_one_is_absent() {
    let mut broken = claimed_pod();
    let annotations = broken.metadata.annotations.as_mut().unwrap();
    annotations.insert("br.sandbox/clean".to_owned(), "maybe".to_owned());
    annotations.insert("br.sandbox/claimed-at".to_owned(), "yesterday".to_owned());
    annotations.insert("br.sandbox/state-bytes".to_owned(), "-5".to_owned());
    annotations.remove("br.sandbox/idle-since");
    let facts = pod_facts(&broken, &PodKeys::default()).unwrap();
    assert_eq!(facts.clean, Mark::Unreadable);
    assert_eq!(facts.claimed_at, Mark::Unreadable);
    assert_eq!(facts.state_bytes, Mark::Unreadable);
    assert_eq!(facts.idle_since, Mark::Absent);
}

#[test]
fn the_names_follow_the_label_prefix() {
    let other = PodKeys::new(br_runner_sandbox::LabelPrefix::new("acme.sbx").unwrap());
    assert!(pod_facts(&claimed_pod(), &other).is_none());
}

#[test]
fn an_owned_deleting_pending_warm_pod_is_read() {
    let warm = pod(json!({
        "metadata": {
            "name": "w", "creationTimestamp": "2026-01-01T00:00:00Z",
            "deletionTimestamp": "2026-01-01T00:05:00Z",
            "labels": {"br.sandbox/pool": "warm"},
            "ownerReferences": [{"apiVersion": "apps/v1", "kind": "ReplicaSet", "name": "rs", "uid": "x"}]
        },
        "status": {"phase": "Pending"}
    }));
    let facts = pod_facts(&warm, &PodKeys::default()).unwrap();
    assert!(facts.owned && facts.deleting && !facts.ready);
    assert_eq!(facts.phase, Phase::Pending);
    assert_eq!(facts.clean, Mark::Absent);
}

#[test]
fn a_replica_set_gives_its_replicas_and_its_conditions() {
    let replica_set: ReplicaSet = serde_json::from_value(json!({
        "metadata": {"name": "rs", "uid": "rs-uid"},
        "spec": {"replicas": 4, "selector": {}},
        "status": {"replicas": 4, "conditions": [{
            "type": "WarmPodsFailing", "status": "False", "reason": "r", "message": "m",
            "lastTransitionTime": "2026-01-01T00:00:00Z"
        }]}
    }))
    .unwrap();
    let facts = replica_set_facts(&replica_set);
    assert_eq!(facts.uid, "rs-uid");
    assert_eq!(facts.replicas, Some(4));
    assert_eq!(facts.conditions[0].kind, "WarmPodsFailing");
    assert_eq!(
        facts.conditions[0].last_transition_time,
        Some(Timestamp::parse("2026-01-01T00:00:00Z").unwrap())
    );
}

fn warm_pod(annotations: serde_json::Value, conditions: serde_json::Value) -> Pod {
    pod(json!({
        "metadata": {
            "name": "w", "creationTimestamp": "2026-01-01T00:00:00Z",
            "labels": {"br.sandbox/pool": "warm"},
            "annotations": annotations
        },
        "status": {"phase": "Pending", "conditions": conditions}
    }))
}

#[test]
fn the_template_hash_annotation_is_read_through_the_label_prefix() {
    let hashed = warm_pod(json!({"br.sandbox/template-hash": "abc123"}), json!([]));
    let facts = pod_facts(&hashed, &PodKeys::default()).unwrap();
    assert_eq!(facts.template_hash, Mark::Value("abc123".to_owned()));
    let other = PodKeys::new(br_runner_sandbox::LabelPrefix::new("acme.sbx").unwrap());
    let mut relabelled = warm_pod(json!({"acme.sbx/template-hash": "zzz"}), json!([]));
    relabelled
        .metadata
        .labels
        .as_mut()
        .unwrap()
        .insert("acme.sbx/pool".to_owned(), "warm".to_owned());
    assert_eq!(
        pod_facts(&relabelled, &other).unwrap().template_hash,
        Mark::Value("zzz".to_owned())
    );
}

#[test]
fn a_missing_template_hash_is_absent_and_an_empty_one_is_unreadable() {
    let bare = warm_pod(json!({}), json!([]));
    assert_eq!(
        pod_facts(&bare, &PodKeys::default()).unwrap().template_hash,
        Mark::Absent
    );
    let empty = warm_pod(json!({"br.sandbox/template-hash": ""}), json!([]));
    assert_eq!(
        pod_facts(&empty, &PodKeys::default())
            .unwrap()
            .template_hash,
        Mark::Unreadable
    );
}

#[test]
fn a_pod_is_unschedulable_when_pod_scheduled_is_false_with_the_reason_unschedulable() {
    let held = warm_pod(
        json!({}),
        json!([{"type": "PodScheduled", "status": "False", "reason": "Unschedulable"}]),
    );
    assert!(pod_facts(&held, &PodKeys::default()).unwrap().unschedulable);
    for conditions in [
        json!([]),
        json!([{"type": "PodScheduled", "status": "True"}]),
        json!([{"type": "PodScheduled", "status": "False", "reason": "SchedulingGated"}]),
        json!([{"type": "Ready", "status": "False", "reason": "Unschedulable"}]),
    ] {
        let pod = warm_pod(json!({}), conditions.clone());
        assert!(
            !pod_facts(&pod, &PodKeys::default()).unwrap().unschedulable,
            "{conditions}"
        );
    }
}
