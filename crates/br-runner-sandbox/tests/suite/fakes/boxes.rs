use std::collections::BTreeMap;
use std::sync::Arc;

use br_agentic_harness_contract::{
    AgentId, InstanceId, OpenRequest, Seed, ThreadRef, TurnId, TurnRef,
};
use br_runner_sandbox::{
    AttemptId, Env, Manifest, PodKeys, PodView, Sandbox, SandboxConfig, SaveKey, manifest_key,
    seed_hash_text,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::common;
use crate::flow_support::encode;
use crate::part_tar::sample_tar;

use super::{FakeStore, ManualClock, PoolCluster, start_time};

pub struct Boxes {
    pub sandbox: Sandbox,
    pub config: Arc<SandboxConfig>,
    pub store: FakeStore,
    pub cluster: PoolCluster,
    pub clock: ManualClock,
}

impl Boxes {
    pub fn clock_now_text(&self) -> String {
        use br_runner_sandbox::Clock;
        self.clock.now().to_string()
    }
}

pub fn boxes_with(extra: Value) -> Boxes {
    let config = Arc::new(common::build(common::with(extra)).expect("a configuration"));
    let store = FakeStore::new();
    let cluster = PoolCluster::new();
    let clock = ManualClock::at(start_time());
    let env = Env::new(
        config.clone(),
        Arc::new(cluster.clone()),
        Arc::new(store.clone()),
        Arc::new(clock.clone()),
    );
    Boxes {
        sandbox: Sandbox::new(env),
        config,
        store,
        cluster,
        clock,
    }
}

pub fn boxes() -> Boxes {
    boxes_with(json!({}))
}

pub async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

pub struct Lineage {
    pub thread: ThreadRef,
    pub agent: AgentId,
}

impl Lineage {
    pub fn new() -> Self {
        Self {
            thread: common::thread(),
            agent: common::agent(),
        }
    }

    pub fn turn(&self, turn: TurnId, parent: Option<TurnId>) -> TurnRef {
        TurnRef::new(self.thread, self.agent, turn, parent)
    }

    pub fn first(&self) -> TurnRef {
        self.turn(common::turn(), None)
    }

    pub fn after(&self, parent: TurnId) -> TurnRef {
        self.turn(common::turn(), Some(parent))
    }
}

pub fn request(turn: TurnRef, seed: Seed) -> OpenRequest {
    let instance = InstanceId::new(Uuid::now_v7()).expect("an instance");
    OpenRequest::new(instance, None, turn, seed)
}

pub fn fresh_request(turn: TurnRef) -> OpenRequest {
    request(turn, Seed::empty())
}

pub fn staged_save(boxes: &Boxes, lineage: &Lineage, parent: TurnId) -> Manifest {
    let key = SaveKey::new(
        boxes.config.client_id().clone(),
        lineage.thread,
        lineage.agent,
        parent,
    );
    let (stored, report) = encode(&sample_tar());
    let manifest = Manifest::new(
        key.clone(),
        None,
        AttemptId::mint(),
        start_time(),
        vec!["/work".to_owned(), "/home/agent".to_owned()],
        report,
    );
    boxes.store.insert(&manifest.part().key, stored);
    boxes
        .store
        .insert(&manifest_key(&key), manifest.to_vec().expect("bytes"));
    manifest
}

pub fn live_pod(name: &str, lineage: &Lineage, parent: TurnId) -> PodView {
    let keys = PodKeys::default();
    let mut pod = super::warm_pod(name);
    pod.labels.insert(keys.pool(), "claimed".to_owned());
    pod.labels.insert(keys.thread(), lineage.thread.to_string());
    pod.labels.insert(keys.agent(), lineage.agent.to_string());
    pod.annotations = BTreeMap::from([
        (keys.clean(), "true".to_owned()),
        (keys.state_turn(), parent.to_string()),
        (keys.saved_at(), start_time().to_string()),
        (keys.idle_since(), start_time().to_string()),
        (keys.state_bytes(), "20000000".to_owned()),
        (keys.seed_hash(), seed_hash_text(&Seed::empty())),
    ]);
    pod
}

pub fn annotate(pod: &mut PodView, key: String, value: impl Into<String>) {
    pod.annotations.insert(key, value.into());
}
