use std::collections::BTreeMap;

use serde_json::Value;

use crate::fault::ImageError;
use crate::keys::CONDITION_WARM_PODS_FAILING;

pub const PHASE_RUNNING: &str = "Running";
pub const CONDITION_TRUE: &str = "True";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodView {
    pub name: String,
    pub resource_version: String,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub phase: Option<String>,
    pub ready_condition: bool,
    pub deleting: bool,
}

impl PodView {
    pub fn is_ready(&self) -> bool {
        self.phase.as_deref() == Some(PHASE_RUNNING) && self.ready_condition && !self.deleting
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionView {
    pub kind: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplicaSetView {
    pub name: String,
    pub conditions: Vec<ConditionView>,
    pub template: Value,
}

impl ReplicaSetView {
    pub fn warm_pods_failing(&self) -> Option<ImageError> {
        self.conditions
            .iter()
            .find(|condition| condition.kind == CONDITION_WARM_PODS_FAILING)
            .filter(|condition| condition.status == CONDITION_TRUE)
            .map(|condition| {
                ImageError::failing(
                    condition.reason.clone().unwrap_or_default(),
                    condition.message.clone().unwrap_or_default(),
                )
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PodEvent {
    Snapshot(Vec<PodView>),
    Applied(PodView),
    Deleted(String),
}
