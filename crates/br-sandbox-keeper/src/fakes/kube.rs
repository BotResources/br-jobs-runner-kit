use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use tokio::sync::mpsc;

use crate::cluster::{
    ApiError, ApiFuture, DeleteOutcome, EventTarget, KubeApi, LeaseFacts, LeaseWrite, PodEvent,
    PoolEvent,
};
use crate::facts::{ConditionFacts, PodFacts, PodVersion, ReplicaSetFacts};
use crate::rules::ConditionWrite;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Delete {
        name: String,
        uid: String,
        resource_version: String,
    },
    Replicas {
        name: String,
        replicas: i32,
    },
    Condition {
        name: String,
        status: String,
        reason: String,
        message: String,
    },
    Event {
        reason: &'static str,
        message: String,
    },
    LeaseCreate {
        holder: String,
    },
    LeaseUpdate {
        holder: String,
        resource_version: String,
    },
}

#[derive(Default)]
struct State {
    namespaces: Vec<String>,
    pods: BTreeMap<(String, String), PodFacts>,
    senders: Vec<(String, mpsc::UnboundedSender<PodEvent>)>,
    replica_sets: BTreeMap<(String, String), ReplicaSetFacts>,
    lease: Option<LeaseFacts>,
    version: u64,
    calls: Vec<Call>,
    failing: HashSet<&'static str>,
    conflicting_leases: u32,
}

#[derive(Default)]
pub struct FakeKube {
    state: Mutex<State>,
}

impl State {
    fn bump(&mut self) -> String {
        self.version += 1;
        self.version.to_string()
    }

    fn emit(&mut self, namespace: &str, event: PodEvent) {
        self.senders
            .retain(|(ns, sender)| ns != namespace || sender.send(event.clone()).is_ok());
    }
}

fn failure(operation: &'static str) -> ApiError {
    ApiError::Call(format!("scripted {operation} failure"))
}

impl FakeKube {
    pub fn set_namespaces(&self, namespaces: &[&str]) {
        self.state.lock().unwrap().namespaces =
            namespaces.iter().map(|ns| (*ns).to_owned()).collect();
    }

    pub fn add_replica_set(&self, namespace: &str, name: &str, replicas: Option<i32>) {
        self.state.lock().unwrap().replica_sets.insert(
            (namespace.to_owned(), name.to_owned()),
            ReplicaSetFacts {
                uid: format!("rs-{name}"),
                replicas,
                conditions: Vec::new(),
            },
        );
    }

    pub fn put_pod(&self, mut pod: PodFacts) -> PodFacts {
        let mut state = self.state.lock().unwrap();
        pod.resource_version = state.bump();
        state
            .pods
            .insert((pod.namespace.clone(), pod.name.clone()), pod.clone());
        state.emit(&pod.namespace.clone(), PodEvent::Applied(pod.clone()));
        pod
    }

    pub fn update_pod(&self, namespace: &str, name: &str, change: impl FnOnce(&mut PodFacts)) {
        let mut state = self.state.lock().unwrap();
        let version = state.bump();
        let pod = state
            .pods
            .get_mut(&(namespace.to_owned(), name.to_owned()))
            .expect("a known pod");
        change(pod);
        pod.resource_version = version;
        let pod = pod.clone();
        state.emit(namespace, PodEvent::Applied(pod));
    }

    pub fn silent_update_pod(
        &self,
        namespace: &str,
        name: &str,
        change: impl FnOnce(&mut PodFacts),
    ) {
        let mut state = self.state.lock().unwrap();
        let version = state.bump();
        let pod = state
            .pods
            .get_mut(&(namespace.to_owned(), name.to_owned()))
            .expect("a known pod");
        change(pod);
        pod.resource_version = version;
    }

    pub fn pod(&self, namespace: &str, name: &str) -> Option<PodFacts> {
        self.state
            .lock()
            .unwrap()
            .pods
            .get(&(namespace.to_owned(), name.to_owned()))
            .cloned()
    }

    pub fn replica_set(&self, namespace: &str, name: &str) -> ReplicaSetFacts {
        self.state
            .lock()
            .unwrap()
            .replica_sets
            .get(&(namespace.to_owned(), name.to_owned()))
            .cloned()
            .expect("a known replica set")
    }

    pub fn set_replica_set_conditions(
        &self,
        namespace: &str,
        name: &str,
        conditions: Vec<ConditionFacts>,
    ) {
        let mut state = self.state.lock().unwrap();
        state
            .replica_sets
            .get_mut(&(namespace.to_owned(), name.to_owned()))
            .expect("a known replica set")
            .conditions = conditions;
    }

    pub fn calls(&self) -> Vec<Call> {
        self.state.lock().unwrap().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.state.lock().unwrap().calls.clear();
    }

    pub fn fail(&self, operation: &'static str) {
        self.state.lock().unwrap().failing.insert(operation);
    }

    pub fn heal(&self, operation: &'static str) {
        self.state.lock().unwrap().failing.remove(operation);
    }

    pub fn lease(&self) -> Option<LeaseFacts> {
        self.state.lock().unwrap().lease.clone()
    }

    pub fn set_lease(&self, lease: Option<LeaseFacts>) {
        self.state.lock().unwrap().lease = lease;
    }

    pub fn conflict_next_lease_writes(&self, count: u32) {
        self.state.lock().unwrap().conflicting_leases = count;
    }

    fn check(&self, operation: &'static str) -> Result<(), ApiError> {
        if self.state.lock().unwrap().failing.contains(operation) {
            Err(failure(operation))
        } else {
            Ok(())
        }
    }

    fn lease_from(write: &LeaseWrite, version: String) -> LeaseFacts {
        LeaseFacts {
            holder: Some(write.holder.clone()),
            acquire_time: Some(write.acquire_time),
            renew_time: Some(write.renew_time),
            duration: Some(write.duration),
            transitions: write.transitions,
            resource_version: version,
        }
    }
}

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
