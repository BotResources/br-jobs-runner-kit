#[cfg(test)]
pub(crate) mod fixtures;
mod kube_api;
mod notice;
mod observe;
mod pool_api;
mod template_owner;
mod view;
mod views;
mod writer;

use std::time::Duration;

use br_runner_sandbox::Timestamp;
use futures::future::BoxFuture;
use futures::stream::BoxStream;

use crate::facts::{PodFacts, PodVersion};
use crate::rules::ConditionWrite;

pub use kube_api::KubeCluster;
pub use notice::{Notice, Watched};
pub use observe::{FenceInputs, FenceObservation, Observer, ReplicaSetRead};
pub use pool_api::{CreateOutcome, PoolApi};
pub use view::{Read, View, ViewEvent};
pub use views::Views;
pub(crate) use writer::{
    APPLY_CONDITION, APPLY_REPLICAS, APPLY_TEMPLATE, CREATE_REPLICA_SET, DELETE_POD,
    DELETE_REPLICA_SET,
};
pub use writer::{Change, PoolWriter, WriteError};

pub(crate) use template_owner::template_in_place;

pub const FIELD_MANAGER: &str = "br-sandbox-keeper";
pub const TEMPLATE_FIELD_MANAGER: &str = "br-sandbox-keeper-template";
pub const EVENT_POOL_AT_MAX: &str = "PoolAtMax";
pub const EVENT_WARM_PODS_PENDING: &str = "WarmPodsPending";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ApiError {
    #[error("the object is gone")]
    NotFound,
    #[error("the object was changed by another writer")]
    Conflict,
    #[error("the API server refused the call: {0}")]
    Forbidden(String),
    #[error("the Kubernetes call failed: {0}")]
    Call(String),
}

pub type ApiFuture<'a, T> = BoxFuture<'a, Result<T, ApiError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeleteOutcome {
    Deleted,
    Gone,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PodEvent {
    Snapshot(Vec<PodFacts>),
    Applied(Box<PodFacts>),
    Deleted { name: String },
    WatchFailed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LeaseFacts {
    pub holder: Option<String>,
    pub acquire_time: Option<Timestamp>,
    pub renew_time: Option<Timestamp>,
    pub duration: Option<Duration>,
    pub transitions: i32,
    pub resource_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LeaseWrite {
    pub holder: String,
    pub acquire_time: Timestamp,
    pub renew_time: Timestamp,
    pub duration: Duration,
    pub transitions: i32,
}

impl LeaseWrite {
    pub fn new(
        holder: impl Into<String>,
        acquire_time: Timestamp,
        renew_time: Timestamp,
        duration: Duration,
        transitions: i32,
    ) -> Self {
        Self {
            holder: holder.into(),
            acquire_time,
            renew_time,
            duration,
            transitions,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EventTarget {
    pub name: String,
    pub uid: String,
}

impl EventTarget {
    pub fn new(name: impl Into<String>, uid: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            uid: uid.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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

pub trait KubeApi: crate::sealed::Sealed + Send + Sync {
    fn managed_namespaces<'a>(&'a self, selector: &'a str) -> ApiFuture<'a, Vec<String>>;

    fn watch_pods(&self, namespace: &str) -> BoxStream<'static, PodEvent>;

    fn get_pod<'a>(&'a self, namespace: &'a str, name: &'a str) -> ApiFuture<'a, Option<PodFacts>>;

    fn delete_pod<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        version: &'a PodVersion,
    ) -> ApiFuture<'a, DeleteOutcome>;

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
