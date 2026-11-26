use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use br_runner_sandbox::{
    Cluster, ClusterError, ClusterFuture, ExecIo, ExecRequest, LabelSelector, PodEvent, PodView,
    ReplicaSetView,
};
use futures::StreamExt;
use futures::stream::BoxStream;
use serde_json::Value;
use tokio::sync::watch;

use super::FakeCluster;
use super::pool_fixtures::pool;

#[derive(Default)]
struct PoolState {
    pods: BTreeMap<String, PodView>,
    replica_set: Option<ReplicaSetView>,
    version: u64,
    created_count: u64,
    ready_on_create: bool,
    created: Vec<Value>,
    patches: Vec<(String, Value)>,
    deleted: Vec<String>,
    calls: Vec<String>,
    failing_deletes: Option<ClusterError>,
    failing_lists: bool,
    failing_replica_sets: bool,
    stale: BTreeSet<String>,
    failing_patch_keys: Vec<String>,
}

#[derive(Clone)]
pub struct PoolCluster {
    state: Arc<Mutex<PoolState>>,
    changes: Arc<watch::Sender<u64>>,
    pub scripts: FakeCluster,
}

fn merge(target: &mut BTreeMap<String, String>, section: Option<&Value>) {
    let Some(entries) = section.and_then(Value::as_object) else {
        return;
    };
    for (key, value) in entries {
        match value.as_str() {
            Some(text) => target.insert(key.clone(), text.to_owned()),
            None => target.remove(key),
        };
    }
}

fn texts(section: Option<&Value>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    merge(&mut out, section);
    out
}

impl PoolCluster {
    pub fn new() -> Self {
        let cluster = Self {
            state: Arc::default(),
            changes: Arc::new(watch::channel(0).0),
            scripts: FakeCluster::new(),
        };
        cluster.lock().ready_on_create = true;
        cluster.lock().replica_set = Some(pool(Vec::new()));
        cluster
    }

    fn lock(&self) -> MutexGuard<'_, PoolState> {
        self.state.lock().expect("the pool lock")
    }

    fn changed(&self) {
        self.changes.send_modify(|count| *count += 1);
    }

    pub fn add_pod(&self, mut pod: PodView) {
        let mut state = self.lock();
        state.version += 1;
        pod.resource_version = state.version.to_string();
        state.pods.insert(pod.name.clone(), pod);
        drop(state);
        self.changed();
    }

    pub fn pod(&self, name: &str) -> Option<PodView> {
        self.lock().pods.get(name).cloned()
    }

    pub fn pod_names(&self) -> Vec<String> {
        self.lock().pods.keys().cloned().collect()
    }

    pub fn set_replica_set(&self, replica_set: Option<ReplicaSetView>) {
        self.lock().replica_set = replica_set;
    }

    pub fn ready_on_create(&self, ready: bool) {
        self.lock().ready_on_create = ready;
    }

    pub fn make_ready(&self, name: &str) {
        if let Some(pod) = self.lock().pods.get_mut(name) {
            pod.phase = Some("Running".to_owned());
            pod.ready_condition = true;
        }
        self.changed();
    }

    pub fn rival_touches(&self, name: &str) {
        self.lock().stale.insert(name.to_owned());
    }

    pub fn fail_deletes(&self, error: ClusterError) {
        self.lock().failing_deletes = Some(error);
    }

    pub fn fail_patches_touching(&self, key: &str) {
        self.lock().failing_patch_keys.push(key.to_owned());
    }

    pub fn fail_lists(&self) {
        self.lock().failing_lists = true;
    }

    pub fn fail_replica_sets(&self) {
        self.lock().failing_replica_sets = true;
    }

    pub fn created(&self) -> Vec<Value> {
        self.lock().created.clone()
    }

    pub fn patches(&self) -> Vec<(String, Value)> {
        self.lock().patches.clone()
    }

    pub fn deleted(&self) -> Vec<String> {
        self.lock().deleted.clone()
    }

    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }
}

impl Cluster for PoolCluster {
    fn list_pods<'a>(&'a self, selector: &'a LabelSelector) -> ClusterFuture<'a, Vec<PodView>> {
        Box::pin(async move {
            let mut state = self.lock();
            state.calls.push(format!("list {selector}"));
            if state.failing_lists {
                return Err(ClusterError::Call(
                    "the API server is unreachable".to_owned(),
                ));
            }
            Ok(state
                .pods
                .values()
                .filter(|pod| selector.matches(&pod.labels))
                .cloned()
                .collect())
        })
    }

    fn get_pod<'a>(&'a self, name: &'a str) -> ClusterFuture<'a, Option<PodView>> {
        Box::pin(async move { Ok(self.pod(name)) })
    }

    fn patch_pod<'a>(&'a self, name: &'a str, patch: &'a Value) -> ClusterFuture<'a, PodView> {
        Box::pin(async move {
            let mut state = self.lock();
            state.calls.push(format!("patch {name}"));
            let text = patch.to_string();
            if state
                .failing_patch_keys
                .iter()
                .any(|key| text.contains(key.as_str()))
            {
                return Err(ClusterError::Call(
                    "the API server refuses this patch".to_owned(),
                ));
            }
            state.patches.push((name.to_owned(), patch.clone()));
            state.version += 1;
            let version = state.version.to_string();
            let stale = state.stale.remove(name);
            let pod = state
                .pods
                .get_mut(name)
                .ok_or_else(|| ClusterError::Gone(name.to_owned()))?;
            let wanted = patch
                .pointer("/metadata/resourceVersion")
                .and_then(Value::as_str);
            if stale || wanted.is_some_and(|wanted| wanted != pod.resource_version) {
                pod.resource_version = version;
                return Err(ClusterError::Conflict(name.to_owned()));
            }
            merge(&mut pod.labels, patch.pointer("/metadata/labels"));
            merge(&mut pod.annotations, patch.pointer("/metadata/annotations"));
            pod.resource_version = version;
            let patched = pod.clone();
            drop(state);
            self.changed();
            Ok(patched)
        })
    }

    fn create_pod<'a>(&'a self, manifest: &'a Value) -> ClusterFuture<'a, PodView> {
        Box::pin(async move {
            let mut state = self.lock();
            state.created_count += 1;
            let name = manifest
                .pointer("/metadata/generateName")
                .and_then(Value::as_str)
                .map_or_else(
                    || "created".to_owned(),
                    |prefix| format!("{prefix}{}", state.created_count),
                );
            state.calls.push(format!("create {name}"));
            state.created.push(manifest.clone());
            state.version += 1;
            let ready = state.ready_on_create;
            let pod = PodView {
                name: name.clone(),
                resource_version: state.version.to_string(),
                labels: texts(manifest.pointer("/metadata/labels")),
                annotations: texts(manifest.pointer("/metadata/annotations")),
                phase: Some(if ready { "Running" } else { "Pending" }.to_owned()),
                ready_condition: ready,
                deleting: false,
            };
            state.pods.insert(name, pod.clone());
            drop(state);
            self.changed();
            Ok(pod)
        })
    }

    fn delete_pod<'a>(&'a self, name: &'a str) -> ClusterFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.lock();
            state.calls.push(format!("delete {name}"));
            if let Some(error) = state.failing_deletes.clone() {
                return Err(error);
            }
            if state.pods.remove(name).is_none() {
                return Err(ClusterError::Gone(name.to_owned()));
            }
            state.deleted.push(name.to_owned());
            drop(state);
            self.changed();
            Ok(())
        })
    }

    fn replica_set<'a>(&'a self, name: &'a str) -> ClusterFuture<'a, Option<ReplicaSetView>> {
        Box::pin(async move {
            let mut state = self.lock();
            state.calls.push(format!("replica_set {name}"));
            if state.failing_replica_sets {
                return Err(ClusterError::Call(
                    "the API server is unreachable".to_owned(),
                ));
            }
            Ok(state.replica_set.clone().filter(|set| set.name == name))
        })
    }

    fn watch_pods<'a>(&'a self, selector: &'a LabelSelector) -> BoxStream<'a, PodEvent> {
        let seen = self.changes.subscribe();
        let start = (seen, self.state.clone(), selector.clone(), true);
        futures::stream::unfold(start, |(mut seen, state, selector, first)| async move {
            if !first {
                seen.changed().await.ok()?;
            }
            let snapshot = state
                .lock()
                .expect("the pool lock")
                .pods
                .values()
                .filter(|pod| selector.matches(&pod.labels))
                .cloned()
                .collect();
            Some((PodEvent::Snapshot(snapshot), (seen, state, selector, false)))
        })
        .boxed()
    }

    fn exec<'a>(&'a self, request: ExecRequest) -> ClusterFuture<'a, ExecIo> {
        self.scripts.exec(request)
    }
}
