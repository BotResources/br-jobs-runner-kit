use futures::stream::BoxStream;
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{DeleteParams, ListParams, Patch, PatchParams, PostParams};
use kube::config::KubeConfigOptions;
use kube::{Api, Client, Config};
use serde_json::Value;

use crate::config::SandboxConfig;

use super::api::{Cluster, ClusterFuture};
use super::error::ClusterError;
use super::exec::{ExecIo, ExecRequest};
use super::kube_convert::{call_error, pod_from_manifest, pod_view, replica_set_view};
use super::kube_exec::start;
use super::kube_watch::watch_pods;
use super::selector::LabelSelector;
use super::views::{PodEvent, PodView, ReplicaSetView};

#[derive(Clone)]
pub struct KubeCluster {
    pods: Api<Pod>,
    replica_sets: Api<ReplicaSet>,
}

fn config_error(error: impl std::fmt::Display) -> ClusterError {
    ClusterError::Config(error.to_string())
}

impl KubeCluster {
    pub fn new(client: Client, namespace: &str) -> Self {
        Self {
            pods: Api::namespaced(client.clone(), namespace),
            replica_sets: Api::namespaced(client, namespace),
        }
    }

    pub async fn connect(config: &SandboxConfig) -> Result<Self, ClusterError> {
        let kube_config = match config.kube_context() {
            Some(context) => {
                let options = KubeConfigOptions {
                    context: Some(context.to_owned()),
                    ..KubeConfigOptions::default()
                };
                Config::from_kubeconfig(&options)
                    .await
                    .map_err(config_error)?
            }
            None => Config::infer().await.map_err(config_error)?,
        };
        crate::tls::ensure_crypto_provider();
        let client = Client::try_from(kube_config).map_err(config_error)?;
        Ok(Self::new(client, config.namespace()))
    }
}

impl Cluster for KubeCluster {
    fn list_pods<'a>(&'a self, selector: &'a LabelSelector) -> ClusterFuture<'a, Vec<PodView>> {
        Box::pin(async move {
            let params = ListParams::default().labels(&selector.to_string());
            let list = self
                .pods
                .list(&params)
                .await
                .map_err(|error| call_error("", error))?;
            Ok(list.items.iter().map(pod_view).collect())
        })
    }

    fn get_pod<'a>(&'a self, name: &'a str) -> ClusterFuture<'a, Option<PodView>> {
        Box::pin(async move {
            let pod = self
                .pods
                .get_opt(name)
                .await
                .map_err(|error| call_error(name, error))?;
            Ok(pod.as_ref().map(pod_view))
        })
    }

    fn patch_pod<'a>(&'a self, name: &'a str, patch: &'a Value) -> ClusterFuture<'a, PodView> {
        Box::pin(async move {
            let pod = self
                .pods
                .patch(name, &PatchParams::default(), &Patch::Merge(patch))
                .await
                .map_err(|error| call_error(name, error))?;
            Ok(pod_view(&pod))
        })
    }

    fn create_pod<'a>(&'a self, manifest: &'a Value) -> ClusterFuture<'a, PodView> {
        Box::pin(async move {
            let pod = pod_from_manifest(manifest)?;
            let created = self
                .pods
                .create(&PostParams::default(), &pod)
                .await
                .map_err(|error| call_error("", error))?;
            Ok(pod_view(&created))
        })
    }

    fn delete_pod<'a>(&'a self, name: &'a str) -> ClusterFuture<'a, ()> {
        Box::pin(async move {
            self.pods
                .delete(name, &DeleteParams::default())
                .await
                .map(|_| ())
                .map_err(|error| call_error(name, error))
        })
    }

    fn replica_set<'a>(&'a self, name: &'a str) -> ClusterFuture<'a, Option<ReplicaSetView>> {
        Box::pin(async move {
            let found = self
                .replica_sets
                .get_opt(name)
                .await
                .map_err(|error| call_error(name, error))?;
            found.as_ref().map(replica_set_view).transpose()
        })
    }

    fn watch_pods<'a>(&'a self, selector: &'a LabelSelector) -> BoxStream<'a, PodEvent> {
        watch_pods(self.pods.clone(), selector)
    }

    fn exec<'a>(&'a self, request: ExecRequest) -> ClusterFuture<'a, ExecIo> {
        Box::pin(async move { start(&self.pods, request).await })
    }
}
