mod kube_api;

use std::time::Duration;

use br_runner_sandbox::Timestamp;
use futures::future::BoxFuture;
use futures::stream::BoxStream;

use crate::facts::{PodFacts, PodVersion, ReplicaSetFacts};
use crate::rules::ConditionWrite;

pub use kube_api::KubeCluster;

pub const FIELD_MANAGER: &str = "br-sandbox-keeper";
pub const EVENT_POOL_AT_MAX: &str = "PoolAtMax";
pub const EVENT_WARM_PODS_PENDING: &str = "WarmPodsPending";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    #[error("the object is gone")]
    NotFound,
    #[error("the object was changed by another writer")]
    Conflict,
    #[error("the Kubernetes call failed: {0}")]
    Call(String),
}

pub type ApiFuture<'a, T> = BoxFuture<'a, Result<T, ApiError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    Gone,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PodEvent {
    Snapshot(Vec<PodFacts>),
    Applied(PodFacts),
    Deleted { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseFacts {
    pub holder: Option<String>,
    pub acquire_time: Option<Timestamp>,
    pub renew_time: Option<Timestamp>,
    pub duration: Option<Duration>,
    pub transitions: i32,
    pub resource_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseWrite {
    pub holder: String,
    pub acquire_time: Timestamp,
    pub renew_time: Timestamp,
    pub duration: Duration,
    pub transitions: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventTarget {
    pub name: String,
    pub uid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolEvent {
    AtMax,
    WarmPodsPending,
}

impl PoolEvent {
    pub fn reason(self) -> &'static str {
        match self {
            PoolEvent::AtMax => EVENT_POOL_AT_MAX,
            PoolEvent::WarmPodsPending => EVENT_WARM_PODS_PENDING,
        }
    }
}

pub trait KubeApi: Send + Sync {
    fn managed_namespaces<'a>(&'a self, selector: &'a str) -> ApiFuture<'a, Vec<String>>;

    fn watch_pods(&self, namespace: &str) -> BoxStream<'static, PodEvent>;

    fn get_pod<'a>(&'a self, namespace: &'a str, name: &'a str) -> ApiFuture<'a, Option<PodFacts>>;

    fn delete_pod<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        version: &'a PodVersion,
    ) -> ApiFuture<'a, DeleteOutcome>;

    fn get_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
    ) -> ApiFuture<'a, Option<ReplicaSetFacts>>;

    fn apply_replicas<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        replicas: i32,
    ) -> ApiFuture<'a, ()>;

    fn apply_condition<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        write: &'a ConditionWrite,
    ) -> ApiFuture<'a, ()>;

    fn create_event<'a>(
        &'a self,
        namespace: &'a str,
        target: &'a EventTarget,
        event: PoolEvent,
        message: &'a str,
    ) -> ApiFuture<'a, ()>;

    fn get_lease<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
    ) -> ApiFuture<'a, Option<LeaseFacts>>;

    fn create_lease<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        write: &'a LeaseWrite,
    ) -> ApiFuture<'a, ()>;

    fn update_lease<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        write: &'a LeaseWrite,
        resource_version: &'a str,
    ) -> ApiFuture<'a, ()>;
}
