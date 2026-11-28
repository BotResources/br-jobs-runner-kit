use std::sync::Arc;

use br_runner_sandbox::PodKeys;
use k8s_openapi::api::apps::v1::ReplicaSet;

use crate::facts::PodVersion;
use crate::metrics::Metrics;
use crate::pool::{
    Owner, OwnershipFault, RenderedTemplate, check_ownership, replica_set_body, replicaset_name,
    template_hash_key,
};
use crate::resource::SandboxPoolStatus;
use crate::rules::ConditionWrite;

use super::{ApiError, CreateOutcome, DeleteOutcome, PoolApi, template_in_place};

pub(crate) const CREATE_REPLICA_SET: &str = "create_replica_set";
pub(crate) const APPLY_REPLICAS: &str = "apply_replicas";
pub(crate) const APPLY_TEMPLATE: &str = "apply_template";
pub(crate) const APPLY_CONDITION: &str = "apply_condition";
pub(crate) const DELETE_REPLICA_SET: &str = "delete_replica_set";
pub(crate) const DELETE_POD: &str = "delete_pod";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WriteError {
    #[error("the API server refused {call}: {reason}")]
    Refused { call: &'static str, reason: String },
    #[error("ReplicaSetNotOwned: {0}")]
    NotOwned(OwnershipFault),
    #[error("{call}: the object is gone")]
    Gone { call: &'static str },
    #[error("{call} failed: {reason}")]
    Failed { call: &'static str, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Change {
    Written,
    Unchanged,
}

#[derive(Clone)]
pub struct PoolWriter {
    api: Arc<dyn PoolApi>,
    metrics: Arc<Metrics>,
    keys: PodKeys,
}

fn replicas_of(value: u32) -> Result<i32, WriteError> {
    i32::try_from(value).map_err(|_| WriteError::Failed {
        call: "replicas",
        reason: format!("{value} does not fit the replicas of a ReplicaSet"),
    })
}

impl PoolWriter {
    pub fn new(api: Arc<dyn PoolApi>, metrics: Arc<Metrics>, keys: PodKeys) -> Self {
        Self { api, metrics, keys }
    }

    fn failed(&self, call: &'static str, namespace: &str, error: ApiError) -> WriteError {
        self.metrics.sweep_error();
        match error {
            ApiError::Forbidden(reason) => {
                tracing::warn!(call, namespace, %reason, "the API server refused the call");
                WriteError::Refused { call, reason }
            }
            ApiError::NotFound => {
                tracing::warn!(call, namespace, "the object is gone");
                WriteError::Gone { call }
            }
            other => {
                tracing::warn!(call, namespace, error = %other, "the call failed");
                WriteError::Failed {
                    call,
                    reason: other.to_string(),
                }
            }
        }
    }

    async fn owned(
        &self,
        call: &'static str,
        namespace: &str,
        owner: &Owner,
    ) -> Result<Option<ReplicaSet>, WriteError> {
        let name = replicaset_name(namespace);
        let found = self
            .api
            .read_replica_set(namespace, &name)
            .await
            .map_err(|error| self.failed(call, namespace, error))?;
        let Some(replica_set) = found else {
            return Ok(None);
        };
        match check_ownership(&replica_set, namespace, owner) {
            Ok(()) => Ok(Some(replica_set)),
            Err(fault) => {
                tracing::warn!(call, namespace, %fault, "the ReplicaSet is not owned by the resource");
                Err(WriteError::NotOwned(fault))
            }
        }
    }

    pub async fn create_replica_set(
        &self,
        namespace: &str,
        owner: &Owner,
        replicas: u32,
        rendered: &RenderedTemplate,
    ) -> Result<CreateOutcome, WriteError> {
        let body = replica_set_body(
            namespace,
            owner,
            replicas_of(replicas)?,
            rendered,
            &self.keys,
        );
        self.api
            .create_replica_set(namespace, &body)
            .await
            .map_err(|error| self.failed(CREATE_REPLICA_SET, namespace, error))
    }

    pub async fn set_replicas(
        &self,
        namespace: &str,
        owner: &Owner,
        desired: u32,
    ) -> Result<Change, WriteError> {
        let call = APPLY_REPLICAS;
        let wanted = replicas_of(desired)?;
        let current = self
            .owned(call, namespace, owner)
            .await?
            .ok_or(WriteError::Gone { call })?;
        if current.spec.as_ref().and_then(|spec| spec.replicas) == Some(wanted) {
            return Ok(Change::Unchanged);
        }
        self.api
            .apply_replicas(namespace, &replicaset_name(namespace), wanted)
            .await
            .map(|()| Change::Written)
            .map_err(|error| self.failed(call, namespace, error))
    }

    pub async fn set_template(
        &self,
        namespace: &str,
        owner: &Owner,
        rendered: &RenderedTemplate,
    ) -> Result<Change, WriteError> {
        let call = APPLY_TEMPLATE;
        let current = self
            .owned(call, namespace, owner)
            .await?
            .ok_or(WriteError::Gone { call })?;
        let key = template_hash_key(self.keys.prefix());
        if template_in_place(&current, &key, &rendered.hash) {
            return Ok(Change::Unchanged);
        }
        self.api
            .apply_template(namespace, &replicaset_name(namespace), &rendered.template)
            .await
            .map(|()| Change::Written)
            .map_err(|error| self.failed(call, namespace, error))
    }

    pub async fn set_condition(
        &self,
        namespace: &str,
        owner: &Owner,
        write: &ConditionWrite,
    ) -> Result<(), WriteError> {
        let call = APPLY_CONDITION;
        let name = replicaset_name(namespace);
        self.owned(call, namespace, owner)
            .await?
            .ok_or(WriteError::Gone { call })?;
        self.api
            .apply_condition(namespace, &name, write)
            .await
            .map_err(|error| self.failed(call, namespace, error))
    }

    pub async fn delete_replica_set(
        &self,
        namespace: &str,
        owner: &Owner,
    ) -> Result<DeleteOutcome, WriteError> {
        let call = DELETE_REPLICA_SET;
        let Some(current) = self.owned(call, namespace, owner).await? else {
            return Ok(DeleteOutcome::Gone);
        };
        let uid = current.metadata.uid.clone().unwrap_or_default();
        if uid.is_empty() {
            return Err(WriteError::Failed {
                call,
                reason: "the ReplicaSet has no uid".to_owned(),
            });
        }
        self.api
            .delete_replica_set(namespace, &replicaset_name(namespace), &uid)
            .await
            .map_err(|error| self.failed(call, namespace, error))
    }

    pub async fn delete_pod(
        &self,
        namespace: &str,
        name: &str,
        version: &PodVersion,
    ) -> Result<DeleteOutcome, WriteError> {
        self.api
            .delete_pod(namespace, name, version)
            .await
            .map_err(|error| self.failed(DELETE_POD, namespace, error))
    }

    pub async fn patch_status(
        &self,
        namespace: &str,
        pool_name: &str,
        status: &SandboxPoolStatus,
    ) -> Result<(), WriteError> {
        self.api
            .patch_pool_status(namespace, pool_name, status)
            .await
            .map_err(|error| self.failed("patch_pool_status", namespace, error))
    }
}

#[cfg(test)]
mod tests;
