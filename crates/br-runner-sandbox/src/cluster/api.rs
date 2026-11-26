use futures::future::BoxFuture;
use futures::stream::BoxStream;
use serde_json::Value;

use super::error::ClusterError;
use super::exec::{ExecIo, ExecRequest};
use super::selector::LabelSelector;
use super::views::{PodEvent, PodView, ReplicaSetView};

pub type ClusterFuture<'a, T> = BoxFuture<'a, Result<T, ClusterError>>;

pub trait Cluster: Send + Sync {
    fn list_pods<'a>(&'a self, selector: &'a LabelSelector) -> ClusterFuture<'a, Vec<PodView>>;

    fn get_pod<'a>(&'a self, name: &'a str) -> ClusterFuture<'a, Option<PodView>>;

    fn patch_pod<'a>(&'a self, name: &'a str, patch: &'a Value) -> ClusterFuture<'a, PodView>;

    fn create_pod<'a>(&'a self, manifest: &'a Value) -> ClusterFuture<'a, PodView>;

    fn delete_pod<'a>(&'a self, name: &'a str) -> ClusterFuture<'a, ()>;

    fn replica_set<'a>(&'a self, name: &'a str) -> ClusterFuture<'a, Option<ReplicaSetView>>;

    fn watch_pods<'a>(&'a self, selector: &'a LabelSelector) -> BoxStream<'a, PodEvent>;

    fn exec<'a>(&'a self, request: ExecRequest) -> ClusterFuture<'a, ExecIo>;
}
