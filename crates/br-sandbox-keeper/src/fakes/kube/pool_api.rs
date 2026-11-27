use std::sync::Arc;

use futures::stream::BoxStream;
use k8s_openapi::api::admissionregistration::v1::ValidatingAdmissionPolicyBinding;
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, ResourceQuota};
use k8s_openapi::api::networking::v1::NetworkPolicy;
use serde_json::Value;

use crate::cluster::{ApiError, ApiFuture, CreateOutcome, DeleteOutcome, PoolApi, ViewEvent};
use crate::pool::Selector;
use crate::resource::{SandboxPool, SandboxPoolStatus};

use super::feed::Filter;
use super::{Call, FakeKube};

use super::pool::CLUSTER;

fn selecting(selector: &str) -> Filter<Namespace> {
    match Selector::parse(selector) {
        Ok(parsed) => Arc::new(move |namespace: &Namespace| {
            parsed.matches(&namespace.metadata.labels.clone().unwrap_or_default())
        }),
        Err(_) => Arc::new(|_: &Namespace| false),
    }
}

impl PoolApi for FakeKube {
    fn watch_namespaces(&self, selector: &str) -> BoxStream<'static, ViewEvent<Namespace>> {
        let mut state = self.state.lock().unwrap();
        state.reads.push("watch_namespaces");
        state
            .namespace_feed
            .open(CLUSTER, Some(selecting(selector)))
    }

    fn watch_bindings(&self) -> BoxStream<'static, ViewEvent<ValidatingAdmissionPolicyBinding>> {
        let mut state = self.state.lock().unwrap();
        state.reads.push("watch_bindings");
        state.binding_feed.open(CLUSTER, None)
    }

    fn watch_pools(&self, namespace: &str) -> BoxStream<'static, ViewEvent<SandboxPool>> {
        let mut state = self.state.lock().unwrap();
        state.reads.push("watch_pools");
        state.pool_feed.open(namespace, None)
    }

    fn read_template<'a>(&'a self, namespace: &'a str) -> ApiFuture<'a, Option<ConfigMap>> {
        Box::pin(async move {
            self.read("read_template", |state| {
                state.templates.get(namespace).cloned()
            })
        })
    }

    fn read_quota<'a>(&'a self, namespace: &'a str) -> ApiFuture<'a, Option<ResourceQuota>> {
        Box::pin(
            async move { self.read("read_quota", |state| state.quotas.get(namespace).cloned()) },
        )
    }

    fn list_network_policies<'a>(
        &'a self,
        namespace: &'a str,
    ) -> ApiFuture<'a, Vec<NetworkPolicy>> {
        Box::pin(async move {
            self.read("list_network_policies", |state| {
                state.policies.get(namespace).cloned().unwrap_or_default()
            })
        })
    }

    fn read_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
    ) -> ApiFuture<'a, Option<ReplicaSet>> {
        Box::pin(async move {
            self.check("read_replica_set")?;
            Ok(self.raw_replica_set(namespace, name))
        })
    }

    fn create_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        body: &'a Value,
    ) -> ApiFuture<'a, CreateOutcome> {
        Box::pin(async move {
            let name = body["metadata"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            self.gated(
                "create_replica_set",
                Call::CreateReplicaSet {
                    name: name.clone(),
                    body: body.clone(),
                },
            )?;
            let mut created: ReplicaSet = serde_json::from_value(body.clone())
                .map_err(|error| ApiError::Call(error.to_string()))?;
            let mut state = self.state.lock().unwrap();
            let key = (namespace.to_owned(), name.clone());
            if state.replica_sets.contains_key(&key) {
                return Ok(CreateOutcome::Exists);
            }
            let version = state.bump();
            created.metadata.uid = Some(format!("rs-{name}-{version}"));
            state.replica_sets.insert(key, created);
            Ok(CreateOutcome::Created)
        })
    }

    fn apply_template<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        template: &'a Value,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            self.gated(
                "apply_template",
                Call::ApplyTemplate {
                    name: name.to_owned(),
                    template: template.clone(),
                },
            )?;
            let parsed = serde_json::from_value(template.clone())
                .map_err(|error| ApiError::Call(error.to_string()))?;
            let mut state = self.state.lock().unwrap();
            match state
                .replica_sets
                .get_mut(&(namespace.to_owned(), name.to_owned()))
            {
                Some(set) => {
                    set.spec.get_or_insert_with(Default::default).template = Some(parsed);
                    Ok(())
                }
                None => Err(ApiError::NotFound),
            }
        })
    }

    fn delete_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        uid: &'a str,
    ) -> ApiFuture<'a, DeleteOutcome> {
        Box::pin(async move {
            self.gated(
                "delete_replica_set",
                Call::DeleteReplicaSet {
                    name: name.to_owned(),
                    uid: uid.to_owned(),
                },
            )?;
            let mut state = self.state.lock().unwrap();
            let key = (namespace.to_owned(), name.to_owned());
            match state.replica_sets.get(&key) {
                None => Ok(DeleteOutcome::Gone),
                Some(set) if set.metadata.uid.as_deref() != Some(uid) => {
                    Ok(DeleteOutcome::Conflict)
                }
                Some(_) => {
                    state.replica_sets.remove(&key);
                    Ok(DeleteOutcome::Deleted)
                }
            }
        })
    }

    fn patch_pool_status<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        status: &'a SandboxPoolStatus,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let value =
                serde_json::to_value(status).map_err(|error| ApiError::Call(error.to_string()))?;
            self.gated(
                "patch_pool_status",
                Call::PoolStatus {
                    name: name.to_owned(),
                    status: value.clone(),
                },
            )?;
            self.state
                .lock()
                .unwrap()
                .statuses
                .insert((namespace.to_owned(), name.to_owned()), value);
            Ok(())
        })
    }
}
