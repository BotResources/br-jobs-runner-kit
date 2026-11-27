use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Mutex;

use tokio::sync::mpsc;

use crate::cluster::{ApiError, LeaseFacts, LeaseWrite, PodEvent};
use crate::facts::{ConditionFacts, PodFacts, ReplicaSetFacts};

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
    lease_targets: BTreeSet<(String, String)>,
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

pub(super) fn failure(operation: &'static str) -> ApiError {
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

    pub fn remove_replica_set(&self, namespace: &str, name: &str) {
        self.state
            .lock()
            .unwrap()
            .replica_sets
            .remove(&(namespace.to_owned(), name.to_owned()));
    }

    pub fn put_pod(&self, mut pod: PodFacts) -> PodFacts {
        let mut state = self.state.lock().unwrap();
        pod.resource_version = state.bump();
        state
            .pods
            .insert((pod.namespace.clone(), pod.name.clone()), pod.clone());
        state.emit(
            &pod.namespace.clone(),
            PodEvent::Applied(Box::new(pod.clone())),
        );
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
        state.emit(namespace, PodEvent::Applied(Box::new(pod)));
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

    pub fn pods_in(&self, namespace: &str) -> Vec<PodFacts> {
        self.state
            .lock()
            .unwrap()
            .pods
            .iter()
            .filter(|((ns, _), _)| ns == namespace)
            .map(|(_, pod)| pod.clone())
            .collect()
    }

    pub fn forget_pod(&self, namespace: &str, name: &str) {
        self.state
            .lock()
            .unwrap()
            .pods
            .remove(&(namespace.to_owned(), name.to_owned()));
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

    pub fn lease_targets(&self) -> Vec<(String, String)> {
        self.state
            .lock()
            .unwrap()
            .lease_targets
            .iter()
            .cloned()
            .collect()
    }

    fn record_lease_target(&self, namespace: &str, name: &str) {
        self.state
            .lock()
            .unwrap()
            .lease_targets
            .insert((namespace.to_owned(), name.to_owned()));
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

mod api;
