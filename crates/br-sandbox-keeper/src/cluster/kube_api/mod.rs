use std::time::UNIX_EPOCH;

use br_runner_sandbox::{PodKeys, Timestamp};
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::api::core::v1::{Event, EventSource, Namespace, ObjectReference, Pod};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
use k8s_openapi::jiff;
use kube::api::{
    DeleteParams, ListParams, Patch, PatchParams, PostParams, Preconditions, WatchParams,
};
use kube::{Api, Client, Config};
use serde_json::json;

use crate::convert::{pod_facts, replica_set_facts};
use crate::facts::{PodFacts, PodVersion, ReplicaSetFacts};
use crate::rules::ConditionWrite;

use super::{
    ApiError, ApiFuture, DeleteOutcome, EventTarget, FIELD_MANAGER, KubeApi, LeaseFacts,
    LeaseWrite, PodEvent, PoolEvent,
};

#[derive(Clone)]
pub struct KubeCluster {
    client: Client,
    keys: PodKeys,
}

fn call_error(error: kube::Error) -> ApiError {
    match &error {
        kube::Error::Api(status) if status.is_not_found() => ApiError::NotFound,
        kube::Error::Api(status) if status.is_conflict() => ApiError::Conflict,
        _ => ApiError::Call(error.to_string()),
    }
}

pub(super) fn instant(time: Timestamp) -> jiff::Timestamp {
    let millis = time
        .to_system_time()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        });
    jiff::Timestamp::from_millisecond(millis).unwrap_or(jiff::Timestamp::UNIX_EPOCH)
}

impl KubeCluster {
    pub fn new(client: Client, keys: PodKeys) -> Self {
        Self { client, keys }
    }

    pub async fn connect(keys: PodKeys) -> Result<Self, ApiError> {
        crate::tls::ensure_crypto_provider();
        let config = Config::infer()
            .await
            .map_err(|error| ApiError::Call(error.to_string()))?;
        let client = Client::try_from(config).map_err(|error| ApiError::Call(error.to_string()))?;
        Ok(Self::new(client, keys))
    }
}

mod lease;
mod watch;

use lease::{lease_facts, lease_object};
use watch::{Phase, Watch, pod_selector};

impl crate::sealed::Sealed for KubeCluster {}

impl KubeApi for KubeCluster {
    fn managed_namespaces<'a>(&'a self, selector: &'a str) -> ApiFuture<'a, Vec<String>> {
        Box::pin(async move {
            let api: Api<Namespace> = Api::all(self.client.clone());
            let list = api
                .list(&ListParams::default().labels(selector))
                .await
                .map_err(call_error)?;
            Ok(list
                .items
                .into_iter()
                .filter_map(|item| item.metadata.name)
                .collect())
        })
    }

    fn watch_pods(&self, namespace: &str) -> BoxStream<'static, PodEvent> {
        let selector = pod_selector(&self.keys);
        let watch = Watch {
            api: Api::namespaced(self.client.clone(), namespace),
            list: ListParams::default().labels(&selector),
            watch: WatchParams::default().labels(&selector),
            keys: self.keys.clone(),
        };
        stream::unfold((watch, Phase::List), |(watch, phase)| async move {
            let (events, phase) = watch.step(phase).await;
            Some((stream::iter(events), (watch, phase)))
        })
        .flatten()
        .boxed()
    }

    fn get_pod<'a>(&'a self, namespace: &'a str, name: &'a str) -> ApiFuture<'a, Option<PodFacts>> {
        Box::pin(async move {
            let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
            let pod = api.get_opt(name).await.map_err(call_error)?;
            Ok(pod.and_then(|pod| pod_facts(&pod, &self.keys)))
        })
    }

    fn delete_pod<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        version: &'a PodVersion,
    ) -> ApiFuture<'a, DeleteOutcome> {
        Box::pin(async move {
            let api: Api<Pod> = Api::namespaced(self.client.clone(), namespace);
            let params = DeleteParams {
                preconditions: Some(Preconditions {
                    uid: Some(version.uid.clone()),
                    resource_version: Some(version.resource_version.clone()),
                }),
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

    fn get_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
    ) -> ApiFuture<'a, Option<ReplicaSetFacts>> {
        Box::pin(async move {
            let api: Api<ReplicaSet> = Api::namespaced(self.client.clone(), namespace);
            let found = api.get_opt(name).await.map_err(call_error)?;
            Ok(found.as_ref().map(replica_set_facts))
        })
    }

    fn apply_replicas<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        replicas: i32,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let api: Api<ReplicaSet> = Api::namespaced(self.client.clone(), namespace);
            let body = json!({
                "apiVersion": "apps/v1",
                "kind": "ReplicaSet",
                "metadata": {"name": name},
                "spec": {"replicas": replicas}
            });
            api.patch(
                name,
                &PatchParams::apply(FIELD_MANAGER).force(),
                &Patch::Apply(body),
            )
            .await
            .map(|_| ())
            .map_err(call_error)
        })
    }

    fn apply_condition<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        write: &'a ConditionWrite,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let api: Api<ReplicaSet> = Api::namespaced(self.client.clone(), namespace);
            let body = json!({
                "apiVersion": "apps/v1",
                "kind": "ReplicaSet",
                "metadata": {"name": name},
                "status": {"conditions": [{
                    "type": br_runner_sandbox::CONDITION_WARM_PODS_FAILING,
                    "status": write.status,
                    "reason": write.reason,
                    "message": write.message,
                    "lastTransitionTime": write.last_transition_time.to_string()
                }]}
            });
            api.patch_status(
                name,
                &PatchParams::apply(FIELD_MANAGER).force(),
                &Patch::Apply(body),
            )
            .await
            .map(|_| ())
            .map_err(call_error)
        })
    }

    fn create_event<'a>(
        &'a self,
        namespace: &'a str,
        target: &'a EventTarget,
        event: PoolEvent,
        message: &'a str,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let api: Api<Event> = Api::namespaced(self.client.clone(), namespace);
            let now = Time(jiff::Timestamp::now());
            let body = Event {
                metadata: ObjectMeta {
                    generate_name: Some(format!("{}.", target.name)),
                    namespace: Some(namespace.to_owned()),
                    ..ObjectMeta::default()
                },
                involved_object: ObjectReference {
                    api_version: Some("apps/v1".to_owned()),
                    kind: Some("ReplicaSet".to_owned()),
                    name: Some(target.name.clone()),
                    namespace: Some(namespace.to_owned()),
                    uid: Some(target.uid.clone()),
                    ..ObjectReference::default()
                },
                reason: Some(event.reason().to_owned()),
                message: Some(message.to_owned()),
                type_: Some("Warning".to_owned()),
                count: Some(1),
                first_timestamp: Some(now.clone()),
                last_timestamp: Some(now),
                reporting_component: Some(FIELD_MANAGER.to_owned()),
                source: Some(EventSource {
                    component: Some(FIELD_MANAGER.to_owned()),
                    ..EventSource::default()
                }),
                ..Event::default()
            };
            api.create(&PostParams::default(), &body)
                .await
                .map(|_| ())
                .map_err(call_error)
        })
    }

    fn get_lease<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
    ) -> ApiFuture<'a, Option<LeaseFacts>> {
        Box::pin(async move {
            let api: Api<Lease> = Api::namespaced(self.client.clone(), namespace);
            let found = api.get_opt(name).await.map_err(call_error)?;
            Ok(found.as_ref().map(lease_facts))
        })
    }

    fn create_lease<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        write: &'a LeaseWrite,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let api: Api<Lease> = Api::namespaced(self.client.clone(), namespace);
            api.create(
                &PostParams::default(),
                &lease_object(name, namespace, write, None),
            )
            .await
            .map(|_| ())
            .map_err(call_error)
        })
    }

    fn update_lease<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        write: &'a LeaseWrite,
        resource_version: &'a str,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let api: Api<Lease> = Api::namespaced(self.client.clone(), namespace);
            let lease = lease_object(name, namespace, write, Some(resource_version));
            api.replace(name, &PostParams::default(), &lease)
                .await
                .map(|_| ())
                .map_err(call_error)
        })
    }
}

#[cfg(test)]
mod tests;
