use std::collections::BTreeMap;
use std::time::{Duration, UNIX_EPOCH};

use br_runner_sandbox::{CLEAN_FALSE, CLEAN_TRUE, POOL_CLAIMED, POOL_WARM, PodKeys, Timestamp};
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::{ContainerStatus, Pod, PodStatus};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;

use crate::pool::template_hash_key;
use crate::facts::{
    ConditionFacts, ContainerFacts, Mark, Phase, PodFacts, PoolState, ReplicaSetFacts, Terminated,
};

const READY: &str = "Ready";
const TRUE: &str = "True";
const FALSE: &str = "False";
const POD_SCHEDULED: &str = "PodScheduled";
const UNSCHEDULABLE: &str = "Unschedulable";

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

pub(crate) fn instant(time: Timestamp) -> jiff::Timestamp {
    let millis = time
        .to_system_time()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        });
    jiff::Timestamp::from_millisecond(millis).unwrap_or(jiff::Timestamp::UNIX_EPOCH)
}

pub(crate) fn time_of(time: Timestamp) -> Time {
    Time(instant(time))
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

fn is_unschedulable(status: Option<&PodStatus>) -> bool {
    status
        .and_then(|status| status.conditions.as_ref())
        .is_some_and(|conditions| {
            conditions.iter().any(|condition| {
                condition.type_ == POD_SCHEDULED
                    && condition.status == FALSE
                    && condition.reason.as_deref() == Some(UNSCHEDULABLE)
            })
        })
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
    let ready = status
        .and_then(|status| status.conditions.as_ref())
        .is_some_and(|conditions| {
            conditions
                .iter()
                .any(|condition| condition.type_ == READY && condition.status == TRUE)
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
        ready,
        been_ready: false,
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
        template_hash: mark(&annotations, &template_hash_key(keys.prefix()), |text| {
            (!text.is_empty()).then(|| text.to_owned())
        }),
        unschedulable: is_unschedulable(status),
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
mod tests;
