use std::sync::Arc;

use br_agentic_harness_contract::{AgentId, ThreadRef, TurnId};
use br_runner_sandbox::{Env, SandboxConfig, SaveKey, Timestamp};
use serde_json::{Value, json};

use crate::common;

use super::{FakeCluster, FakeStore, ManualClock};

pub const POD: &str = "warm-main-abc12";

pub struct Rig {
    pub env: Env,
    pub config: Arc<SandboxConfig>,
    pub store: FakeStore,
    pub cluster: FakeCluster,
    pub clock: ManualClock,
}

pub struct Ids {
    pub thread: ThreadRef,
    pub agent: AgentId,
    pub turn: TurnId,
    pub parent: TurnId,
}

impl Ids {
    pub fn new() -> Self {
        Self {
            thread: common::thread(),
            agent: common::agent(),
            turn: common::turn(),
            parent: common::turn(),
        }
    }

    pub fn key(&self, rig: &Rig) -> SaveKey {
        SaveKey::new(
            rig.config.client_id().clone(),
            self.thread,
            self.agent,
            self.turn,
        )
    }

    pub fn parent_key(&self, rig: &Rig) -> SaveKey {
        SaveKey::new(
            rig.config.client_id().clone(),
            self.thread,
            self.agent,
            self.parent,
        )
    }
}

pub fn start_time() -> Timestamp {
    Timestamp::parse("2026-11-26T10:00:00Z").expect("a time")
}

pub fn rig_with(extra: Value) -> Rig {
    let config = Arc::new(common::build(common::with(extra)).expect("a configuration"));
    let store = FakeStore::new();
    let cluster = FakeCluster::new();
    let clock = ManualClock::at(start_time());
    cluster.add_pod(super::pod(POD));
    let env = Env::new(
        config.clone(),
        Arc::new(cluster.clone()),
        Arc::new(store.clone()),
        Arc::new(clock.clone()),
    );
    Rig {
        env,
        config,
        store,
        cluster,
        clock,
    }
}

pub fn rig() -> Rig {
    rig_with(json!({}))
}
