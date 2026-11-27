use futures::StreamExt;
use futures::stream::{self, BoxStream};
use tokio::sync::mpsc;

use crate::cluster::{
    ApiError, ApiFuture, DeleteOutcome, EventTarget, KubeApi, LeaseFacts, LeaseWrite, PodEvent,
    PoolEvent,
};
use crate::facts::{ConditionFacts, PodFacts, PodVersion, ReplicaSetFacts};
use crate::rules::ConditionWrite;

use super::{Call, FakeKube, failure};

impl crate::sealed::Sealed for FakeKube {}

impl KubeApi for FakeKube {
    fn managed_namespaces<'a>(&'a self, _selector: &'a str) -> ApiFuture<'a, Vec<String>> {
        Box::pin(async move {
            self.check("namespaces")?;
            Ok(self.state.lock().unwrap().namespaces.clone())
        })
    }

    fn watch_pods(&self, namespace: &str) -> BoxStream<'static, PodEvent> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut state = self.state.lock().unwrap();
        let snapshot: Vec<PodFacts> = state
            .pods
            .iter()
            .filter(|((ns, _), _)| ns == namespace)
            .map(|(_, pod)| pod.clone())
            .collect();
        sender.send(PodEvent::Snapshot(snapshot)).unwrap();
        state.senders.push((namespace.to_owned(), sender));
        stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        })
        .boxed()
    }

    fn get_pod<'a>(&'a self, namespace: &'a str, name: &'a str) -> ApiFuture<'a, Option<PodFacts>> {
        Box::pin(async move {
            self.check("get_pod")?;
            Ok(self.pod(namespace, name))
        })
    }

    fn delete_pod<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        version: &'a PodVersion,
    ) -> ApiFuture<'a, DeleteOutcome> {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            state.calls.push(Call::Delete {
                name: name.to_owned(),
                uid: version.uid.clone(),
                resource_version: version.resource_version.clone(),
            });
            if state.failing.contains("delete_pod") {
                return Err(failure("delete_pod"));
            }
            let key = (namespace.to_owned(), name.to_owned());
            match state.pods.get(&key) {
                None => Ok(DeleteOutcome::Gone),
                Some(pod)
                    if pod.uid != version.uid
                        || pod.resource_version != version.resource_version =>
                {
                    Ok(DeleteOutcome::Conflict)
                }
                Some(_) => {
                    state.pods.remove(&key);
                    state.emit(
                        namespace,
                        PodEvent::Deleted {
                            name: name.to_owned(),
                        },
                    );
                    Ok(DeleteOutcome::Deleted)
                }
            }
        })
    }

    fn get_replica_set<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
    ) -> ApiFuture<'a, Option<ReplicaSetFacts>> {
        Box::pin(async move {
            self.check("get_replica_set")?;
            Ok(self
                .state
                .lock()
                .unwrap()
                .replica_sets
                .get(&(namespace.to_owned(), name.to_owned()))
                .cloned())
        })
    }

    fn apply_replicas<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        replicas: i32,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            state.calls.push(Call::Replicas {
                name: name.to_owned(),
                replicas,
            });
            if state.failing.contains("apply_replicas") {
                return Err(failure("apply_replicas"));
            }
            if let Some(set) = state
                .replica_sets
                .get_mut(&(namespace.to_owned(), name.to_owned()))
            {
                set.replicas = Some(replicas);
            }
            Ok(())
        })
    }

    fn apply_condition<'a>(
        &'a self,
        namespace: &'a str,
        name: &'a str,
        write: &'a ConditionWrite,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            state.calls.push(Call::Condition {
                name: name.to_owned(),
                status: write.status.to_owned(),
                reason: write.reason.to_owned(),
                message: write.message.clone(),
            });
            if state.failing.contains("apply_condition") {
                return Err(failure("apply_condition"));
            }
            if let Some(set) = state
                .replica_sets
                .get_mut(&(namespace.to_owned(), name.to_owned()))
            {
                set.conditions
                    .retain(|condition| condition.kind != "WarmPodsFailing");
                set.conditions.push(ConditionFacts {
                    kind: "WarmPodsFailing".to_owned(),
                    status: write.status.to_owned(),
                    reason: Some(write.reason.to_owned()),
                    message: Some(write.message.clone()),
                    last_transition_time: Some(write.last_transition_time),
                });
            }
            Ok(())
        })
    }

    fn create_event<'a>(
        &'a self,
        _namespace: &'a str,
        _target: &'a EventTarget,
        event: PoolEvent,
        message: &'a str,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            state.calls.push(Call::Event {
                reason: event.reason(),
                message: message.to_owned(),
            });
            if state.failing.contains("create_event") {
                return Err(failure("create_event"));
            }
            Ok(())
        })
    }

    fn get_lease<'a>(
        &'a self,
        _namespace: &'a str,
        _name: &'a str,
    ) -> ApiFuture<'a, Option<LeaseFacts>> {
        Box::pin(async move {
            self.check("get_lease")?;
            Ok(self.lease())
        })
    }

    fn create_lease<'a>(
        &'a self,
        _namespace: &'a str,
        _name: &'a str,
        write: &'a LeaseWrite,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            state.calls.push(Call::LeaseCreate {
                holder: write.holder.clone(),
            });
            if state.failing.contains("create_lease") {
                return Err(failure("create_lease"));
            }
            if state.lease.is_some() {
                return Err(ApiError::Conflict);
            }
            let version = state.bump();
            state.lease = Some(Self::lease_from(write, version));
            Ok(())
        })
    }

    fn update_lease<'a>(
        &'a self,
        _namespace: &'a str,
        _name: &'a str,
        write: &'a LeaseWrite,
        resource_version: &'a str,
    ) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            state.calls.push(Call::LeaseUpdate {
                holder: write.holder.clone(),
                resource_version: resource_version.to_owned(),
            });
            if state.failing.contains("update_lease") {
                return Err(failure("update_lease"));
            }
            if state.conflicting_leases > 0 {
                state.conflicting_leases -= 1;
                return Err(ApiError::Conflict);
            }
            let current = state
                .lease
                .as_ref()
                .map(|lease| lease.resource_version.clone());
            if current.as_deref() != Some(resource_version) {
                return Err(ApiError::Conflict);
            }
            let version = state.bump();
            state.lease = Some(Self::lease_from(write, version));
            Ok(())
        })
    }
}
