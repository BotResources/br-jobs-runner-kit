use std::collections::HashSet;
use std::time::Duration;

use br_agentic_harness_contract::TurnRef;

use crate::cluster::{ClusterError, ReplicaSetView};
use crate::decide::warm_claim_patch;
use crate::error::Error;
use crate::fault::ImageError;

use super::guard::OpenGuard;
use super::state::Sandbox;

enum Warm {
    Claimed(String),
    NoReadyPod,
    AttemptsSpent,
}

pub(super) fn image_failure(error: ImageError) -> Error {
    match &error {
        ImageError::Failing { reason, message } => {
            tracing::warn!(%reason, %message, "the warm pods of the pool do not start");
        }
        ImageError::Unavailable { waited } => {
            let waited_ms = u64::try_from(waited.as_millis()).unwrap_or(u64::MAX);
            tracing::warn!(waited_ms, "no box was ready in the time that open allows");
        }
    }
    Error::Image(error)
}

impl Sandbox {
    pub(super) async fn pool(&self) -> Result<ReplicaSetView, Error> {
        let name = self.env().config.pool_id().replica_set_name();
        self.env()
            .cluster
            .replica_set(&name)
            .await?
            .ok_or(Error::PoolMissing(name))
    }

    pub(super) async fn image_error(&self, waited: Duration) -> Error {
        match self.pool().await {
            Ok(pool) => image_failure(
                pool.warm_pods_failing()
                    .unwrap_or_else(|| ImageError::unavailable(waited)),
            ),
            Err(error) => error,
        }
    }

    pub(super) async fn remove_pod(&self, name: &str) -> Result<(), Error> {
        match self.env().cluster.delete_pod(name).await {
            Ok(()) | Err(ClusterError::Gone(_)) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) async fn discard(&self, guard: &mut OpenGuard) {
        if let Some(pod) = guard.pod() {
            let _ = self.remove_pod(&pod).await;
            guard.forget_pod();
        }
    }

    pub(super) async fn claim_pod(
        &self,
        turn: &TurnRef,
        guard: &mut OpenGuard,
    ) -> Result<String, Error> {
        let config = &self.env().config;
        self.inner.warm.wait_synced(config.open_timeout()).await;
        let pool = match self.claim_warm(turn, guard).await? {
            Warm::Claimed(pod) => return Ok(pod),
            Warm::NoReadyPod => {
                let pool = self.pool().await?;
                if let Some(failing) = pool.warm_pods_failing() {
                    return Err(image_failure(failing));
                }
                pool
            }
            Warm::AttemptsSpent => self.pool().await?,
        };
        if !config.cold_create() {
            return Err(self.image_error(Duration::ZERO).await);
        }
        self.cold_pod(turn, &pool, guard).await
    }

    async fn claim_warm(&self, turn: &TurnRef, guard: &mut OpenGuard) -> Result<Warm, Error> {
        let env = self.env();
        let keys = env.pod_keys();
        let attempts = usize::try_from(env.config.claim_max_attempts()).unwrap_or(usize::MAX);
        let mut tried = HashSet::new();
        while tried.len() < attempts {
            let Some(pod) = self.inner.warm.pick(&tried) else {
                return Ok(Warm::NoReadyPod);
            };
            tried.insert(pod.name.clone());
            let patch = warm_claim_patch(
                &keys,
                &pod.resource_version,
                turn.thread,
                turn.agent,
                env.clock.now(),
            );
            let claimed = env.cluster.patch_pod(&pod.name, &patch).await;
            self.inner.warm.forget(&pod.name);
            match claimed {
                Ok(_) => {
                    guard.hold(pod.name.clone());
                    return Ok(Warm::Claimed(pod.name));
                }
                Err(ClusterError::Conflict(_) | ClusterError::Gone(_)) => {}
                Err(other) => return Err(other.into()),
            }
        }
        Ok(Warm::AttemptsSpent)
    }
}
