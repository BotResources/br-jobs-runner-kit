use k8s_openapi::api::admissionregistration::v1::ValidatingAdmissionPolicyBinding;
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, ResourceQuota};
use k8s_openapi::api::networking::v1::NetworkPolicy;
use serde_json::Value;

use crate::cluster::{ApiError, PodEvent};
use crate::resource::SandboxPool;

use super::state::State;
use super::{Call, FakeKube};

pub(super) const CLUSTER: &str = "";

impl FakeKube {
    pub fn put_namespace(&self, namespace: Namespace) {
        self.state
            .lock()
            .unwrap()
            .namespace_feed
            .put(CLUSTER, namespace);
    }

    pub fn remove_namespace(&self, name: &str) {
        self.state
            .lock()
            .unwrap()
            .namespace_feed
            .remove(CLUSTER, name);
    }

    pub fn put_binding(&self, binding: ValidatingAdmissionPolicyBinding) {
        self.state
            .lock()
            .unwrap()
            .binding_feed
            .put(CLUSTER, binding);
    }

    pub fn remove_binding(&self, name: &str) {
        self.state
            .lock()
            .unwrap()
            .binding_feed
            .remove(CLUSTER, name);
    }

    pub fn put_pool(&self, namespace: &str, mut pool: SandboxPool) {
        let mut state = self.state.lock().unwrap();
        if pool.status.is_none() {
            pool.status = state
                .pool_feed
                .values(namespace)
                .into_iter()
                .find(|held| held.metadata.name == pool.metadata.name)
                .and_then(|held| held.status);
        }
        state.pool_feed.put(namespace, pool);
    }

    pub fn remove_pool(&self, namespace: &str, name: &str) {
        self.state.lock().unwrap().pool_feed.remove(namespace, name);
    }

    pub fn put_template(&self, namespace: &str, config_map: ConfigMap) {
        self.state
            .lock()
            .unwrap()
            .templates
            .insert(namespace.to_owned(), config_map);
    }

    pub fn remove_template(&self, namespace: &str) {
        self.state.lock().unwrap().templates.remove(namespace);
    }

    pub fn remove_quota(&self, namespace: &str) {
        self.state.lock().unwrap().quotas.remove(namespace);
    }

    pub fn race_next_create(&self, replica_set: ReplicaSet) {
        self.state.lock().unwrap().race = Some(replica_set);
    }

    pub fn put_quota(&self, namespace: &str, quota: ResourceQuota) {
        self.state
            .lock()
            .unwrap()
            .quotas
            .insert(namespace.to_owned(), quota);
    }

    pub fn put_network_policies(&self, namespace: &str, policies: Vec<NetworkPolicy>) {
        self.state
            .lock()
            .unwrap()
            .policies
            .insert(namespace.to_owned(), policies);
    }

    pub fn break_watch(&self, kind: &str, namespace: &str, reason: &str) {
        let mut state = self.state.lock().unwrap();
        match kind {
            "namespaces" => state.namespace_feed.fail(CLUSTER, reason),
            "bindings" => state.binding_feed.fail(CLUSTER, reason),
            "pools" => state.pool_feed.fail(namespace, reason),
            "pods" => {
                let event = PodEvent::WatchFailed {
                    reason: reason.to_owned(),
                };
                state.emit(namespace, event);
            }
            other => panic!("no watch of {other}"),
        }
    }

    pub fn remove_definition(&self, namespace: &str) {
        self.state
            .lock()
            .unwrap()
            .pool_feed
            .missing(namespace, "the SandboxPool definition is absent");
    }

    pub fn resync_watch(&self, kind: &str, namespace: &str) {
        let mut state = self.state.lock().unwrap();
        match kind {
            "namespaces" => state.namespace_feed.resync(CLUSTER),
            "bindings" => state.binding_feed.resync(CLUSTER),
            "pools" => state.pool_feed.resync(namespace),
            "pods" => {
                let snapshot = state
                    .pods
                    .iter()
                    .filter(|((ns, _), _)| ns == namespace)
                    .map(|(_, pod)| pod.clone())
                    .collect();
                state.emit(namespace, PodEvent::Snapshot(snapshot));
            }
            other => panic!("no watch of {other}"),
        }
    }

    pub fn pool_status(&self, namespace: &str, name: &str) -> Option<Value> {
        self.state
            .lock()
            .unwrap()
            .statuses
            .get(&(namespace.to_owned(), name.to_owned()))
            .cloned()
    }

    pub fn watch_opens(&self, kind: &str, namespace: &str) -> usize {
        let state = self.state.lock().unwrap();
        match kind {
            "namespaces" => state.namespace_feed.opens(CLUSTER),
            "bindings" => state.binding_feed.opens(CLUSTER),
            "pools" => state.pool_feed.opens(namespace),
            other => panic!("no counted watch of {other}"),
        }
    }

    pub(super) fn read<T>(
        &self,
        operation: &'static str,
        found: impl FnOnce(&State) -> T,
    ) -> Result<T, ApiError> {
        self.check(operation)?;
        Ok(found(&self.state.lock().unwrap()))
    }

    pub(super) fn gated(&self, operation: &'static str, call: Call) -> Result<(), ApiError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push(call);
        match state.refusal(operation) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
