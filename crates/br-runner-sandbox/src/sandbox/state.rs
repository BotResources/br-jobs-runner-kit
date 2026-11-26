use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use br_agentic_harness_contract::{InstanceId, TurnRef};
use tokio::task::JoinHandle;

use crate::clock::SystemClock;
use crate::cluster::{KubeCluster, LabelSelector};
use crate::config::SandboxConfig;
use crate::env::Env;
use crate::error::Error;
use crate::keys::POOL_WARM;
use crate::store::S3Store;

use super::warm::WarmPods;

#[derive(Debug, Clone, Copy)]
pub(super) struct BoxRecord {
    pub instance: InstanceId,
    pub turn: TurnRef,
    pub saved_bytes: Option<u64>,
    pub released: bool,
}

#[derive(Default)]
pub(super) struct State {
    pub instances: HashSet<InstanceId>,
    pub boxes: HashMap<String, BoxRecord>,
}

pub(super) struct Inner {
    pub env: Env,
    pub warm: Arc<WarmPods>,
    state: Mutex<State>,
    watcher: JoinHandle<()>,
}

impl Inner {
    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}

#[derive(Clone)]
pub struct Sandbox {
    pub(super) inner: Arc<Inner>,
}

impl Sandbox {
    pub fn new(env: Env) -> Self {
        let keys = env.pod_keys();
        let selector = LabelSelector::new()
            .with(keys.pool(), POOL_WARM)
            .with(keys.pool_id(), env.config.pool_id().as_str());
        let warm = Arc::new(WarmPods::new(selector));
        let watcher = warm.spawn(env.cluster.clone());
        Self {
            inner: Arc::new(Inner {
                env,
                warm,
                state: Mutex::new(State::default()),
                watcher,
            }),
        }
    }

    pub async fn connect(config: SandboxConfig) -> Result<Self, Error> {
        let cluster = KubeCluster::connect(&config).await?;
        let store = S3Store::new(config.store())?;
        Ok(Self::new(Env::new(
            Arc::new(config),
            Arc::new(cluster),
            Arc::new(store),
            Arc::new(SystemClock),
        )))
    }

    pub(super) fn env(&self) -> &Env {
        &self.inner.env
    }
}
