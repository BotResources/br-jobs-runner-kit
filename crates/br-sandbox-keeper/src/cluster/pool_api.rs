use futures::stream::BoxStream;
use k8s_openapi::api::admissionregistration::v1::ValidatingAdmissionPolicyBinding;
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, ResourceQuota};
use k8s_openapi::api::networking::v1::NetworkPolicy;
use serde_json::Value;

use crate::resource::{SandboxPool, SandboxPoolStatus};

use super::{ApiFuture, DeleteOutcome, KubeApi, ViewEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CreateOutcome {
    Created,
    Exists,
}

pub trait PoolApi: KubeApi {
    fn watch_namespaces(&self, selector: &str) -> BoxStream<'static, ViewEvent<Namespace>>;

    fn watch_bindings(&self) -> BoxStream<'static, ViewEvent<ValidatingAdmissionPolicyBinding>>;

    fn watch_pools(&self, namespace: &str) -> BoxStream<'static, ViewEvent<SandboxPool>>;

    fn read_template<'a>(&'a self, namespace: &'a str) -> ApiFuture<'a, Option<ConfigMap>>;

    fn read_quota<'a>(&'a self, namespace: &'a str) -> ApiFuture<'a, Option<ResourceQuota>>;

    fn list_network_policies<'a>(&'a self, namespace: &'a str)
    -> ApiFuture<'a, Vec<NetworkPolicy>>;

    fn read_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
    ) -> ApiFuture<'a, Option<ReplicaSet>>;

    fn create_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        body: &'a Value,
    ) -> ApiFuture<'a, CreateOutcome>;

    fn apply_template<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        template: &'a Value,
    ) -> ApiFuture<'a, ()>;

    fn delete_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        uid: &'a str,
    ) -> ApiFuture<'a, DeleteOutcome>;

    fn patch_pool_status<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        status: &'a SandboxPoolStatus,
    ) -> ApiFuture<'a, ()>;
}
