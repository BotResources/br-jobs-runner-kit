use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Status;
use serde_json::Value;

use super::error::ClusterError;
use super::views::{CONDITION_TRUE, ConditionView, PodView, ReplicaSetView};

const READY_CONDITION: &str = "Ready";
const NON_ZERO_EXIT_REASON: &str = "NonZeroExitCode";
const EXIT_CODE_CAUSE: &str = "ExitCode";
const SUCCESS: &str = "Success";
const FAILURE: &str = "Failure";

pub(super) fn pod_view(pod: &Pod) -> PodView {
    let ready_condition = pod
        .status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .is_some_and(|conditions| {
            conditions.iter().any(|condition| {
                condition.type_ == READY_CONDITION && condition.status == CONDITION_TRUE
            })
        });
    PodView {
        name: pod.metadata.name.clone().unwrap_or_default(),
        resource_version: pod.metadata.resource_version.clone().unwrap_or_default(),
        labels: pod.metadata.labels.clone().unwrap_or_default(),
        annotations: pod.metadata.annotations.clone().unwrap_or_default(),
        phase: pod.status.as_ref().and_then(|status| status.phase.clone()),
        ready_condition,
        deleting: pod.metadata.deletion_timestamp.is_some(),
    }
}

pub(super) fn replica_set_view(replica_set: &ReplicaSet) -> Result<ReplicaSetView, ClusterError> {
    let template = replica_set
        .spec
        .as_ref()
        .and_then(|spec| spec.template.as_ref())
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| ClusterError::Call(error.to_string()))?
        .unwrap_or(Value::Null);
    let conditions = replica_set
        .status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .map(|conditions| {
            conditions
                .iter()
                .map(|condition| ConditionView {
                    kind: condition.type_.clone(),
                    status: condition.status.clone(),
                    reason: condition.reason.clone(),
                    message: condition.message.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ReplicaSetView {
        name: replica_set.metadata.name.clone().unwrap_or_default(),
        conditions,
        template,
    })
}

pub(super) fn exit_code(status: &Status) -> Option<i32> {
    match status.status.as_deref() {
        Some(SUCCESS) => Some(0),
        Some(FAILURE) if status.reason.as_deref() == Some(NON_ZERO_EXIT_REASON) => status
            .details
            .as_ref()?
            .causes
            .as_ref()?
            .iter()
            .find(|cause| cause.reason.as_deref() == Some(EXIT_CODE_CAUSE))
            .and_then(|cause| cause.message.as_deref()?.trim().parse().ok()),
        _ => None,
    }
}

pub(super) fn pod_from_manifest(manifest: &Value) -> Result<Pod, ClusterError> {
    serde_json::from_value(manifest.clone())
        .map_err(|error| ClusterError::Config(error.to_string()))
}

pub(super) fn call_error(name: &str, error: kube::Error) -> ClusterError {
    match &error {
        kube::Error::Api(status) if status.is_not_found() => ClusterError::Gone(name.to_owned()),
        kube::Error::Api(status) if status.is_conflict() => ClusterError::Conflict(name.to_owned()),
        _ => ClusterError::Call(error.to_string()),
    }
}
