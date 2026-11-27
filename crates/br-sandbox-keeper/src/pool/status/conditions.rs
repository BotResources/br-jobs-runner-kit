use br_runner_sandbox::{CONDITION_WARM_PODS_FAILING, Timestamp};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;

use super::decide::{Observation, ReplicaSetView};
use super::{
    CONDITION_READY, REASON_NOT_OBSERVED, REASON_PODS_REFUSED, REASON_REPLICASET_ABSENT,
    REASON_REPLICASET_NOT_OWNED, REASON_WARM_BELOW_MINIMUM, REASON_WARM_PODS_READY,
};
use crate::convert::time_of;
use crate::rules::failing_status;

pub(super) struct Fields {
    pub status: &'static str,
    pub reason: &'static str,
    pub message: String,
}

impl Fields {
    pub fn new(status: &'static str, reason: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            reason,
            message: message.into(),
        }
    }
}

pub(super) fn ready_fields(observation: Option<&Observation>, min_replicas: Option<u32>) -> Fields {
    let Some(observation) = observation else {
        return Fields::new(
            "Unknown",
            REASON_NOT_OBSERVED,
            "the pod view is not synced or a read failed",
        );
    };
    let refusal = match &observation.replica_set {
        ReplicaSetView::Absent => {
            return Fields::new(
                "False",
                REASON_REPLICASET_ABSENT,
                "the ReplicaSet does not exist",
            );
        }
        ReplicaSetView::NotOwned => {
            return Fields::new(
                "False",
                REASON_REPLICASET_NOT_OWNED,
                "a ReplicaSet of the name of the pool exists and this resource does not own it",
            );
        }
        ReplicaSetView::Owned { refusal } => refusal,
    };
    let Some(minimum) = min_replicas else {
        return Fields::new(
            "Unknown",
            REASON_NOT_OBSERVED,
            "the minimum of the pool is not known",
        );
    };
    let ready = observation.counts.ready;
    if ready >= minimum {
        return Fields::new(
            "True",
            REASON_WARM_PODS_READY,
            format!("{ready} warm pod(s) are Ready, the minimum is {minimum}"),
        );
    }
    match refusal {
        Some(message) => Fields::new("False", REASON_PODS_REFUSED, message.clone()),
        None => Fields::new(
            "False",
            REASON_WARM_BELOW_MINIMUM,
            format!("{ready} warm pod(s) are Ready, the minimum is {minimum}"),
        ),
    }
}

pub(super) fn failing_fields(observation: &Observation) -> Fields {
    Fields::new(
        failing_status(&observation.failing),
        observation.failing.reason,
        observation.failing.message.clone(),
    )
}

pub(super) const FAILING_TYPE: &str = CONDITION_WARM_PODS_FAILING;
pub(super) const READY_TYPE: &str = CONDITION_READY;

pub(super) fn build(
    kind: &str,
    fields: Fields,
    previous: &[Condition],
    generation: Option<i64>,
    now: Timestamp,
) -> Condition {
    let transition = previous
        .iter()
        .find(|condition| condition.type_ == kind && condition.status == fields.status)
        .map_or_else(
            || time_of(now),
            |condition| condition.last_transition_time.clone(),
        );
    Condition {
        type_: kind.to_owned(),
        status: fields.status.to_owned(),
        reason: fields.reason.to_owned(),
        message: fields.message,
        last_transition_time: transition,
        observed_generation: generation,
    }
}
