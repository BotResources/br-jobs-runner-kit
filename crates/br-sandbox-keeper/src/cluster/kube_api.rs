use std::time::{Duration, UNIX_EPOCH};

use br_runner_sandbox::{PodKeys, Timestamp};
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use k8s_openapi::api::apps::v1::ReplicaSet;
use k8s_openapi::api::coordination::v1::{Lease, LeaseSpec};
use k8s_openapi::api::core::v1::{Event, EventSource, Namespace, ObjectReference, Pod};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{MicroTime, ObjectMeta, Time};
use k8s_openapi::jiff;
use kube::api::{
    DeleteParams, ListParams, Patch, PatchParams, PostParams, Preconditions, WatchEvent,
    WatchParams,
};
use kube::{Api, Client, Config};
use serde_json::json;

use crate::convert::{pod_facts, replica_set_facts, timestamp_of};
use crate::facts::{PodFacts, PodVersion, ReplicaSetFacts};
use crate::rules::ConditionWrite;

use super::{
    ApiError, ApiFuture, DeleteOutcome, EventTarget, FIELD_MANAGER, KubeApi, LeaseFacts,
    LeaseWrite, PodEvent, PoolEvent,
};

const WATCH_RETRY: Duration = Duration::from_secs(1);

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

fn instant(time: Timestamp) -> jiff::Timestamp {
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

fn pod_selector(keys: &PodKeys) -> String {
    format!(
        "{} in ({},{})",
        keys.pool(),
        br_runner_sandbox::POOL_WARM,
        br_runner_sandbox::POOL_CLAIMED
    )
}

enum Phase {
    List,
    Watch {
        version: String,
        events: BoxStream<'static, kube::Result<WatchEvent<Pod>>>,
    },
}

struct Watch {
    api: Api<Pod>,
    list: ListParams,
    watch: WatchParams,
    keys: PodKeys,
}

fn non_empty(candidate: Option<&String>, previous: String) -> String {
    candidate
        .filter(|text| !text.is_empty())
        .cloned()
        .unwrap_or(previous)
}

impl Watch {
    async fn open(
        &self,
        version: &str,
    ) -> Option<BoxStream<'static, kube::Result<WatchEvent<Pod>>>> {
        self.api
            .watch(&self.watch, version)
            .await
            .ok()
            .map(StreamExt::boxed)
    }

    async fn pause_then_relist(&self) -> (Vec<PodEvent>, Phase) {
        tokio::time::sleep(WATCH_RETRY).await;
        (Vec::new(), Phase::List)
    }

    async fn relist(&self) -> (Vec<PodEvent>, Phase) {
        let Ok(list) = self.api.list(&self.list).await else {
            return self.pause_then_relist().await;
        };
        let version = list.metadata.resource_version.clone().unwrap_or_default();
        let facts = list
            .items
            .iter()
            .filter_map(|pod| pod_facts(pod, &self.keys))
            .collect();
        let snapshot = PodEvent::Snapshot(facts);
        match self.open(&version).await {
            Some(events) => (vec![snapshot], Phase::Watch { version, events }),
            None => {
                tokio::time::sleep(WATCH_RETRY).await;
                (vec![snapshot], Phase::List)
            }
        }
    }

    async fn step(&self, phase: Phase) -> (Vec<PodEvent>, Phase) {
        let Phase::Watch {
            version,
            mut events,
        } = phase
        else {
            return self.relist().await;
        };
        match events.next().await {
            Some(Ok(WatchEvent::Added(pod) | WatchEvent::Modified(pod))) => {
                let version = non_empty(pod.metadata.resource_version.as_ref(), version);
                let applied =
                    pod_facts(&pod, &self.keys).map(|facts| PodEvent::Applied(Box::new(facts)));
                (
                    applied.into_iter().collect(),
                    Phase::Watch { version, events },
                )
            }
            Some(Ok(WatchEvent::Deleted(pod))) => {
                let version = non_empty(pod.metadata.resource_version.as_ref(), version);
                let name = pod.metadata.name.clone().unwrap_or_default();
                (
                    vec![PodEvent::Deleted { name }],
                    Phase::Watch { version, events },
                )
            }
            Some(Ok(WatchEvent::Bookmark(bookmark))) => (
                Vec::new(),
                Phase::Watch {
                    version: bookmark.metadata.resource_version,
                    events,
                },
            ),
            Some(Ok(WatchEvent::Error(_)) | Err(_)) => self.pause_then_relist().await,
            None => match self.open(&version).await {
                Some(events) => (Vec::new(), Phase::Watch { version, events }),
                None => self.pause_then_relist().await,
            },
        }
    }
}

fn lease_facts(lease: &Lease) -> LeaseFacts {
    let spec = lease.spec.clone().unwrap_or_default();
    LeaseFacts {
        holder: spec.holder_identity.filter(|holder| !holder.is_empty()),
        acquire_time: spec
            .acquire_time
            .as_ref()
            .map(|time| timestamp_of(&Time(time.0))),
        renew_time: spec
            .renew_time
            .as_ref()
            .map(|time| timestamp_of(&Time(time.0))),
        duration: spec
            .lease_duration_seconds
            .and_then(|seconds| u64::try_from(seconds).ok())
            .map(Duration::from_secs),
        transitions: spec.lease_transitions.unwrap_or(0),
        resource_version: lease.metadata.resource_version.clone().unwrap_or_default(),
    }
}

fn lease_object(name: &str, namespace: &str, write: &LeaseWrite, version: Option<&str>) -> Lease {
    Lease {
        metadata: ObjectMeta {
            name: Some(name.to_owned()),
            namespace: Some(namespace.to_owned()),
            resource_version: version.map(str::to_owned),
            ..ObjectMeta::default()
        },
        spec: Some(LeaseSpec {
            holder_identity: Some(write.holder.clone()),
            acquire_time: Some(MicroTime(instant(write.acquire_time))),
            renew_time: Some(MicroTime(instant(write.renew_time))),
            lease_duration_seconds: i32::try_from(write.duration.as_secs()).ok(),
            lease_transitions: Some(write.transitions),
            ..LeaseSpec::default()
        }),
    }
}

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
mod tests {
    use super::*;
    use crate::time::at;

    #[test]
    fn a_lease_goes_through_the_object_and_back() {
        let write = LeaseWrite {
            holder: "keeper-a".to_owned(),
            acquire_time: at(100),
            renew_time: at(200),
            duration: Duration::from_secs(15),
            transitions: 3,
        };
        let lease = lease_object("br-sandbox-keeper", "sys", &write, Some("42"));
        let facts = lease_facts(&lease);
        assert_eq!(facts.holder.as_deref(), Some("keeper-a"));
        assert_eq!(facts.renew_time, Some(at(200)));
        assert_eq!(facts.duration, Some(Duration::from_secs(15)));
        assert_eq!(facts.transitions, 3);
        assert_eq!(facts.resource_version, "42");
    }

    #[test]
    fn a_lease_with_an_empty_holder_has_no_holder() {
        let mut lease = lease_object(
            "n",
            "s",
            &LeaseWrite {
                holder: "x".to_owned(),
                acquire_time: at(1),
                renew_time: at(1),
                duration: Duration::from_secs(1),
                transitions: 0,
            },
            None,
        );
        lease.spec.as_mut().unwrap().holder_identity = Some(String::new());
        assert_eq!(lease_facts(&lease).holder, None);
    }

    #[test]
    fn the_pod_selector_names_the_two_pool_states_under_the_prefix() {
        assert_eq!(
            pod_selector(&PodKeys::default()),
            "br.sandbox/pool in (warm,claimed)"
        );
        let other = PodKeys::new(br_runner_sandbox::LabelPrefix::new("acme.sbx").unwrap());
        assert_eq!(pod_selector(&other), "acme.sbx/pool in (warm,claimed)");
    }
}
