use std::collections::BTreeMap;
use std::time::{Duration, UNIX_EPOCH};

use br_runner_sandbox::{CLEAN_FALSE, CLEAN_TRUE, POOL_CLAIMED, POOL_WARM, PodKeys, Timestamp};
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::{ContainerStatus, Pod};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;

use crate::facts::{
    ConditionFacts, ContainerFacts, Mark, Phase, PodFacts, PoolState, ReplicaSetFacts, Terminated,
};

const READY: &str = "Ready";
const TRUE: &str = "True";

pub(crate) fn timestamp_of(time: &Time) -> Timestamp {
    let millis = time.0.as_millisecond();
    let base = UNIX_EPOCH;
    let moment = if millis >= 0 {
        base + Duration::from_millis(millis as u64)
    } else {
        base - Duration::from_millis(millis.unsigned_abs())
    };
    Timestamp::from_system_time(moment)
}

fn mark<T>(
    annotations: &BTreeMap<String, String>,
    key: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> Mark<T> {
    match annotations.get(key) {
        None => Mark::Absent,
        Some(text) => parse(text).map_or(Mark::Unreadable, Mark::Value),
    }
}

fn parse_clean(text: &str) -> Option<bool> {
    match text {
        CLEAN_TRUE => Some(true),
        CLEAN_FALSE => Some(false),
        _ => None,
    }
}

fn parse_bytes(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn phase_of(text: Option<&str>) -> Phase {
    match text {
        Some("Pending") => Phase::Pending,
        Some("Running") => Phase::Running,
        Some("Succeeded") => Phase::Succeeded,
        Some("Failed") => Phase::Failed,
        _ => Phase::Unknown,
    }
}

fn container_facts(status: &ContainerStatus) -> ContainerFacts {
    let state = status.state.as_ref();
    ContainerFacts {
        waiting_reason: state
            .and_then(|state| state.waiting.as_ref())
            .and_then(|waiting| waiting.reason.clone()),
        terminated: state
            .and_then(|state| state.terminated.as_ref())
            .map(|terminated| Terminated {
                exit_code: terminated.exit_code,
                reason: terminated.reason.clone(),
            }),
    }
}

pub fn pod_facts(pod: &Pod, keys: &PodKeys) -> Option<PodFacts> {
    let labels = pod.metadata.labels.clone().unwrap_or_default();
    let annotations = pod.metadata.annotations.clone().unwrap_or_default();
    let pool_state = match labels.get(&keys.pool()).map(String::as_str) {
        Some(POOL_WARM) => PoolState::Warm,
        Some(POOL_CLAIMED) => PoolState::Claimed,
        _ => return None,
    };
    let status = pod.status.as_ref();
    let statuses = status.into_iter().flat_map(|status| {
        status
            .init_container_statuses
            .iter()
            .flatten()
            .chain(status.container_statuses.iter().flatten())
    });
    Some(PodFacts {
        namespace: pod.metadata.namespace.clone().unwrap_or_default(),
        name: pod.metadata.name.clone().unwrap_or_default(),
        uid: pod.metadata.uid.clone().unwrap_or_default(),
        resource_version: pod.metadata.resource_version.clone().unwrap_or_default(),
        pool_state,
        pool_id: labels.get(&keys.pool_id()).cloned(),
        thread: labels.get(&keys.thread()).cloned(),
        agent: labels.get(&keys.agent()).cloned(),
        phase: phase_of(status.and_then(|status| status.phase.as_deref())),
        created_at: pod
            .metadata
            .creation_timestamp
            .as_ref()
            .map_or_else(|| Timestamp::from_system_time(UNIX_EPOCH), timestamp_of),
        ready: status
            .and_then(|status| status.conditions.as_ref())
            .is_some_and(|conditions| {
                conditions
                    .iter()
                    .any(|condition| condition.type_ == READY && condition.status == TRUE)
            }),
        owned: pod
            .metadata
            .owner_references
            .as_ref()
            .is_some_and(|owners| !owners.is_empty()),
        deleting: pod.metadata.deletion_timestamp.is_some(),
        clean: mark(&annotations, &keys.clean(), parse_clean),
        claimed_at: mark(&annotations, &keys.claimed_at(), |text| {
            Timestamp::parse(text).ok()
        }),
        idle_since: mark(&annotations, &keys.idle_since(), |text| {
            Timestamp::parse(text).ok()
        }),
        state_bytes: mark(&annotations, &keys.state_bytes(), parse_bytes),
        containers: statuses.map(container_facts).collect(),
    })
}

pub fn replica_set_facts(replica_set: &ReplicaSet) -> ReplicaSetFacts {
    ReplicaSetFacts {
        uid: replica_set.metadata.uid.clone().unwrap_or_default(),
        replicas: replica_set.spec.as_ref().and_then(|spec| spec.replicas),
        conditions: replica_set
            .status
            .as_ref()
            .and_then(|status| status.conditions.as_ref())
            .into_iter()
            .flatten()
            .map(|condition| ConditionFacts {
                kind: condition.type_.clone(),
                status: condition.status.clone(),
                reason: condition.reason.clone(),
                message: condition.message.clone(),
                last_transition_time: condition.last_transition_time.as_ref().map(timestamp_of),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
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
        let bare =
            pod(json!({"metadata": {"name": "proxy", "labels": {"br.sandbox/role": "proxy"}}}));
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
}
