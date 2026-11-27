use std::sync::Mutex;

use k8s_openapi::api::apps::v1::ReplicaSet;

use crate::cluster::{ApiError, LeaseFacts, LeaseWrite, PodEvent};
use crate::facts::{ConditionFacts, PodFacts};

pub use call::Call;
use state::{State, condition_of};

pub struct FakeKube {
    state: Mutex<State>,
}

impl FakeKube {
    pub fn put_replica_set(&self, replica_set: ReplicaSet) {
        let key = (
            replica_set.metadata.namespace.clone().unwrap_or_default(),
            replica_set.metadata.name.clone().unwrap_or_default(),
        );
        self.state
            .lock()
            .unwrap()
            .replica_sets
            .insert(key, replica_set);
    }

    pub fn raw_replica_set(&self, namespace: &str, name: &str) -> Option<ReplicaSet> {
        self.state
            .lock()
            .unwrap()
            .replica_sets
            .get(&(namespace.to_owned(), name.to_owned()))
            .cloned()
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

    pub fn remove_pod(&self, namespace: &str, name: &str) {
        let mut state = self.state.lock().unwrap();
        state.pods.remove(&(namespace.to_owned(), name.to_owned()));
        state.emit(
            namespace,
            PodEvent::Deleted {
                name: name.to_owned(),
            },
        );
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

    pub fn set_replica_set_conditions(
        &self,
        namespace: &str,
        name: &str,
        conditions: Vec<ConditionFacts>,
    ) {
        let mut state = self.state.lock().unwrap();
        let set = state
            .replica_sets
            .get_mut(&(namespace.to_owned(), name.to_owned()))
            .expect("a known replica set");
        set.status.get_or_insert_with(Default::default).conditions =
            Some(conditions.iter().map(condition_of).collect());
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
        let mut state = self.state.lock().unwrap();
        state.reads.push(operation);
        match state.refusal(operation) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub fn forbid(&self, operation: &'static str) {
        self.state.lock().unwrap().forbidden.insert(operation);
    }

    pub fn permit(&self, operation: &'static str) {
        self.state.lock().unwrap().forbidden.remove(operation);
    }

    pub fn reads(&self) -> Vec<&'static str> {
        self.state.lock().unwrap().reads.clone()
    }

    pub fn clear_reads(&self) {
        self.state.lock().unwrap().reads.clear();
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
mod call;
mod feed;
mod ownership;
mod pool;
mod pool_api;
mod state;
