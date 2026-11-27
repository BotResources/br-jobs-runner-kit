use std::sync::Arc;

use futures::StreamExt;
use futures::stream::BoxStream;
use k8s_openapi::api::admissionregistration::v1::ValidatingAdmissionPolicyBinding;
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::core::v1::{ConfigMap, Namespace, ResourceQuota};
use k8s_openapi::api::networking::v1::NetworkPolicy;
use kube::Api;
use kube::api::{
    DeleteParams, ListParams, Patch, PatchParams, PostParams, Preconditions, PropagationPolicy,
    WatchParams,
};
use kube::core::Request;
use serde_json::{Value, json};

use crate::pool::{ADMISSION_BINDING_NAME, QUOTA_NAME};
use crate::resource::{SandboxPool, SandboxPoolStatus};
use crate::rights::POD_TEMPLATE_CONFIGMAP;

use crate::cluster::{CreateOutcome, PoolApi, TEMPLATE_FIELD_MANAGER, ViewEvent};

use super::watch::watch_stream;
use super::{ApiError, ApiFuture, DeleteOutcome, FIELD_MANAGER, KubeCluster, call_error};

const STATUS_FIELDS: [&str; 7] = [
    "observedGeneration",
    "conditions",
    "desiredReplicas",
    "replicas",
    "readyReplicas",
    "updatedReplicas",
    "observedAt",
];

pub(super) fn status_patch(status: &SandboxPoolStatus) -> Result<Value, ApiError> {
    let mut written = serde_json::to_value(status)
        .map_err(|error| ApiError::Call(error.to_string()))?
        .as_object()
        .cloned()
        .unwrap_or_default();
    for field in STATUS_FIELDS {
        written.entry(field).or_insert(Value::Null);
    }
    Ok(json!({"status": written}))
}

fn is_already_exists(error: &kube::Error) -> bool {
    matches!(error, kube::Error::Api(status) if status.is_already_exists())
}

impl PoolApi for KubeCluster {
    fn watch_namespaces(&self, selector: &str) -> BoxStream<'static, ViewEvent<Namespace>> {
        watch_stream(
            Api::<Namespace>::all(self.client.clone()),
            ListParams::default().labels(selector),
            WatchParams::default().labels(selector),
            "namespace",
            Arc::new(|namespace: &Namespace| Some(namespace.clone())),
        )
        .boxed()
    }

    fn watch_bindings(&self) -> BoxStream<'static, ViewEvent<ValidatingAdmissionPolicyBinding>> {
        let field = format!("metadata.name={ADMISSION_BINDING_NAME}");
        watch_stream(
            Api::<ValidatingAdmissionPolicyBinding>::all(self.client.clone()),
            ListParams::default().fields(&field),
            WatchParams::default().fields(&field),
            "admission binding",
            Arc::new(|binding: &ValidatingAdmissionPolicyBinding| Some(binding.clone())),
        )
        .boxed()
    }

    fn watch_pools(&self, namespace: &str) -> BoxStream<'static, ViewEvent<SandboxPool>> {
        watch_stream(
            Api::<SandboxPool>::namespaced(self.client.clone(), namespace),
            ListParams::default(),
            WatchParams::default(),
            "pool resource",
            Arc::new(|pool: &SandboxPool| Some(pool.clone())),
        )
        .boxed()
    }

    fn read_template<'a>(&'a self, namespace: &'a str) -> ApiFuture<'a, Option<ConfigMap>> {
        Box::pin(async move {
            let api: Api<ConfigMap> = Api::namespaced(self.client.clone(), namespace);
            api.get_opt(POD_TEMPLATE_CONFIGMAP)
                .await
                .map_err(call_error)
        })
    }

    fn read_quota<'a>(&'a self, namespace: &'a str) -> ApiFuture<'a, Option<ResourceQuota>> {
        Box::pin(async move {
            let api: Api<ResourceQuota> = Api::namespaced(self.client.clone(), namespace);
            api.get_opt(QUOTA_NAME).await.map_err(call_error)
        })
    }

    fn list_network_policies<'a>(
        &'a self,
        namespace: &'a str,
    ) -> ApiFuture<'a, Vec<NetworkPolicy>> {
        Box::pin(async move {
            let api: Api<NetworkPolicy> = Api::namespaced(self.client.clone(), namespace);
            let list = api.list(&ListParams::default()).await.map_err(call_error)?;
            Ok(list.items)
        })
    }

    fn read_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
    ) -> ApiFuture<'a, Option<ReplicaSet>> {
        Box::pin(async move {
            let api: Api<ReplicaSet> = Api::namespaced(self.client.clone(), namespace);
            api.get_opt(name).await.map_err(call_error)
        })
    }

    fn create_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        body: &'a Value,
    ) -> ApiFuture<'a, CreateOutcome> {
        Box::pin(async move {
            let data =
                serde_json::to_vec(body).map_err(|error| ApiError::Call(error.to_string()))?;
            let params = PostParams {
                field_manager: Some(FIELD_MANAGER.to_owned()),
                ..PostParams::default()
            };
            let request = Request::new(format!("/apis/apps/v1/namespaces/{namespace}/replicasets"))
                .create(&params, data)
                .map_err(|error| ApiError::Call(error.to_string()))?;
            match self.client.request::<ReplicaSet>(request).await {
                Ok(_) => Ok(CreateOutcome::Created),
                Err(error) if is_already_exists(&error) => Ok(CreateOutcome::Exists),
                Err(error) => Err(call_error(error)),
            }
        })
    }

    fn apply_template<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        template: &'a Value,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let api: Api<ReplicaSet> = Api::namespaced(self.client.clone(), namespace);
            let body = json!({
                "apiVersion": "apps/v1",
                "kind": "ReplicaSet",
                "metadata": {"name": name},
                "spec": {"template": template}
            });
            api.patch(
                name,
                &PatchParams::apply(TEMPLATE_FIELD_MANAGER).force(),
                &Patch::Apply(body),
            )
            .await
            .map(|_| ())
            .map_err(call_error)
        })
    }

    fn delete_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        uid: &'a str,
    ) -> ApiFuture<'a, DeleteOutcome> {
        Box::pin(async move {
            let api: Api<ReplicaSet> = Api::namespaced(self.client.clone(), namespace);
            let params = DeleteParams {
                preconditions: Some(Preconditions {
                    uid: Some(uid.to_owned()),
                    resource_version: None,
                }),
                propagation_policy: Some(PropagationPolicy::Background),
                ..DeleteParams::default()
            };
            match api.delete(name, &params).await {
                Ok(_) => Ok(DeleteOutcome::Deleted),
                Err(error) => match call_error(error) {
                    ApiError::NotFound => Ok(DeleteOutcome::Gone),
                    ApiError::Conflict => Ok(DeleteOutcome::Conflict),
                    other => Err(other),
                },
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
            let api: Api<SandboxPool> = Api::namespaced(self.client.clone(), namespace);
            let body = status_patch(status)?;
            api.patch_status(name, &PatchParams::default(), &Patch::Merge(body))
                .await
                .map(|_| ())
                .map_err(call_error)
        })
    }
}
