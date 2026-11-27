use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Mutex;

use k8s_openapi::api::admissionregistration::v1::ValidatingAdmissionPolicyBinding;
use k8s_openapi::api::apps::v1::{ReplicaSet, ReplicaSetCondition};
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, ResourceQuota};
use k8s_openapi::api::networking::v1::NetworkPolicy;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::cluster::{ApiError, LeaseFacts, PodEvent};
use crate::convert::time_of;
use crate::facts::{ConditionFacts, PodFacts};
use crate::resource::SandboxPool;

use super::FakeKube;
use super::call::Call;
use super::feed::Feed;

pub(super) struct State {
    pub(super) pods: BTreeMap<(String, String), PodFacts>,
    pub(super) senders: Vec<(String, mpsc::UnboundedSender<PodEvent>)>,
    pub(super) replica_sets: BTreeMap<(String, String), ReplicaSet>,
    pub(super) namespace_feed: Feed<Namespace>,
    pub(super) binding_feed: Feed<ValidatingAdmissionPolicyBinding>,
    pub(super) pool_feed: Feed<SandboxPool>,
    pub(super) templates: BTreeMap<String, ConfigMap>,
    pub(super) quotas: BTreeMap<String, ResourceQuota>,
    pub(super) policies: BTreeMap<String, Vec<NetworkPolicy>>,
    pub(super) statuses: BTreeMap<(String, String), Value>,
    pub(super) reads: Vec<&'static str>,
    pub(super) forbidden: HashSet<&'static str>,
    pub(super) lease: Option<LeaseFacts>,
    pub(super) lease_targets: BTreeSet<(String, String)>,
    pub(super) version: u64,
    pub(super) calls: Vec<Call>,
    pub(super) failing: HashSet<&'static str>,
    pub(super) conflicting_leases: u32,
    pub(super) race: Option<ReplicaSet>,
}

impl Default for FakeKube {
    fn default() -> Self {
        Self {
            state: Mutex::new(State {
                pods: BTreeMap::new(),
                senders: Vec::new(),
                replica_sets: BTreeMap::new(),
                namespace_feed: Feed::new(|namespace: &Namespace| {
                    namespace.metadata.name.clone().unwrap_or_default()
                }),
                binding_feed: Feed::new(|binding: &ValidatingAdmissionPolicyBinding| {
                    binding.metadata.name.clone().unwrap_or_default()
                }),
                pool_feed: Feed::new(|pool: &SandboxPool| {
                    pool.metadata.name.clone().unwrap_or_default()
                }),
                templates: BTreeMap::new(),
                quotas: BTreeMap::new(),
                policies: BTreeMap::new(),
                statuses: BTreeMap::new(),
                reads: Vec::new(),
                forbidden: HashSet::new(),
                lease: None,
                lease_targets: BTreeSet::new(),
                version: 0,
                calls: Vec::new(),
                failing: HashSet::new(),
                conflicting_leases: 0,
                race: None,
            }),
        }
    }
}

impl State {
    pub(super) fn refusal(&self, operation: &'static str) -> Option<ApiError> {
        if self.forbidden.contains(operation) {
            Some(ApiError::Forbidden(format!("scripted {operation} refusal")))
        } else if self.failing.contains(operation) {
            Some(failure(operation))
        } else {
            None
        }
    }

    pub(super) fn bump(&mut self) -> String {
        self.version += 1;
        self.version.to_string()
    }

    pub(super) fn emit(&mut self, namespace: &str, event: PodEvent) {
        self.senders
            .retain(|(ns, sender)| ns != namespace || sender.send(event.clone()).is_ok());
    }
}

pub(super) fn condition_of(condition: &ConditionFacts) -> ReplicaSetCondition {
    ReplicaSetCondition {
        type_: condition.kind.clone(),
        status: condition.status.clone(),
        reason: condition.reason.clone(),
        message: condition.message.clone(),
        last_transition_time: condition.last_transition_time.map(time_of),
    }
}

pub(super) fn failure(operation: &'static str) -> ApiError {
    ApiError::Call(format!("scripted {operation} failure"))
}
