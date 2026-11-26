use std::sync::Arc;

use crate::clock::Clock;
use crate::cluster::{Cluster, ExecRequest};
use crate::config::SandboxConfig;
use crate::keys::PodKeys;
use crate::store::Store;

#[derive(Clone)]
pub struct Env {
    pub config: Arc<SandboxConfig>,
    pub cluster: Arc<dyn Cluster>,
    pub store: Arc<dyn Store>,
    pub clock: Arc<dyn Clock>,
}

impl Env {
    pub fn new(
        config: Arc<SandboxConfig>,
        cluster: Arc<dyn Cluster>,
        store: Arc<dyn Store>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            config,
            cluster,
            store,
            clock,
        }
    }

    pub fn pod_keys(&self) -> PodKeys {
        PodKeys::new(self.config.label_prefix().clone())
    }

    pub fn agent_request(&self, pod: &str, command: Vec<String>, stdin: bool) -> ExecRequest {
        ExecRequest {
            pod: pod.to_owned(),
            container: self.config.agent_container().to_owned(),
            command,
            stdin,
        }
    }

    pub fn seed_request(&self, pod: &str, command: Vec<String>) -> ExecRequest {
        ExecRequest {
            pod: pod.to_owned(),
            container: self.config.seed_container().to_owned(),
            command,
            stdin: true,
        }
    }
}
