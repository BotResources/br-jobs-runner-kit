use std::collections::BTreeMap;
use std::sync::Arc;

use br_runner_sandbox::PodKeys;
use k8s_openapi::api::admissionregistration::v1::ValidatingAdmissionPolicyBinding;
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::ResourceQuota;
use k8s_openapi::api::networking::v1::NetworkPolicy;

use crate::pool::{
    FenceFacts, FenceVerdict, Owner, OwnershipFault, PoolTemplate, TemplateFault, check_ownership,
    fence_verdict, replicaset_name,
};

use super::{PoolApi, Read, Views};

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FenceInputs {
    pub namespace: String,
    pub labels: BTreeMap<String, String>,
    pub quotas: Vec<ResourceQuota>,
    pub network_policies: Vec<NetworkPolicy>,
    pub binding: Option<ValidatingAdmissionPolicyBinding>,
}

impl FenceInputs {
    pub fn verdict(&self, managed_selector: &str) -> FenceVerdict {
        fence_verdict(&FenceFacts::new(
            &self.namespace,
            &self.labels,
            managed_selector,
            &self.quotas,
            &self.network_policies,
            self.binding.as_ref(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FenceObservation {
    Observed(Box<FenceInputs>),
    NotManaged,
    NotObserved(String),
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ReplicaSetRead {
    Absent,
    Owned(Box<ReplicaSet>),
    NotOwned(OwnershipFault),
}

#[derive(Clone)]
pub struct Observer {
    api: Arc<dyn PoolApi>,
    views: Views,
}

fn unobserved<T>(what: &str, error: impl std::fmt::Display) -> Read<T> {
    Read::not_observed(format!("{what} could not be read: {error}"))
}

impl Observer {
    pub fn new(api: Arc<dyn PoolApi>, views: Views) -> Self {
        Self { api, views }
    }

    pub async fn template(
        &self,
        namespace: &str,
        keys: &PodKeys,
        agent_container: &str,
    ) -> Read<Result<PoolTemplate, TemplateFault>> {
        match self.api.read_template(namespace).await {
            Ok(config_map) => Read::Observed(PoolTemplate::read(
                config_map.as_ref(),
                namespace,
                keys,
                agent_container,
            )),
            Err(error) => unobserved("the ConfigMap of the pod template", error),
        }
    }

    pub async fn fence(&self, namespace: &str) -> FenceObservation {
        let labels = match self.views.namespace(namespace) {
            Read::NotObserved(reason) => return FenceObservation::NotObserved(reason),
            Read::Observed(None) => return FenceObservation::NotManaged,
            Read::Observed(Some(found)) => found.metadata.labels.unwrap_or_default(),
        };
        let binding = match self.views.binding() {
            Read::NotObserved(reason) => return FenceObservation::NotObserved(reason),
            Read::Observed(found) => found,
        };
        let quotas = match self.api.read_quota(namespace).await {
            Ok(found) => found.into_iter().collect(),
            Err(error) => {
                return FenceObservation::NotObserved(format!(
                    "the ResourceQuota could not be read: {error}"
                ));
            }
        };
        let network_policies = match self.api.list_network_policies(namespace).await {
            Ok(found) => found,
            Err(error) => {
                return FenceObservation::NotObserved(format!(
                    "the NetworkPolicies could not be listed: {error}"
                ));
            }
        };
        FenceObservation::Observed(Box::new(FenceInputs {
            namespace: namespace.to_owned(),
            labels,
            quotas,
            network_policies,
            binding,
        }))
    }

    pub async fn replica_set(&self, namespace: &str, owner: &Owner) -> Read<ReplicaSetRead> {
        let name = replicaset_name(namespace);
        match self.api.read_replica_set(namespace, &name).await {
            Err(error) => unobserved("the ReplicaSet", error),
            Ok(None) => Read::Observed(ReplicaSetRead::Absent),
            Ok(Some(found)) => Read::Observed(match check_ownership(&found, namespace, owner) {
                Ok(()) => ReplicaSetRead::Owned(Box::new(found)),
                Err(fault) => ReplicaSetRead::NotOwned(fault),
            }),
        }
    }
}

#[cfg(test)]
mod tests;
