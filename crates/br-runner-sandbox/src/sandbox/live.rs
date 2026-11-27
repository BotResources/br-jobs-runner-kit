use br_agentic_harness_contract::{TurnId, TurnRef};

use crate::cluster::{ClusterError, LabelSelector, PodView};
use crate::decide::{LivePolicy, LiveVerdict, PodMarks, judge_live_pod, live_claim_patch};
use crate::error::Error;
use crate::keys::POOL_CLAIMED;
use crate::time::Timestamp;

use super::guard::OpenGuard;
use super::state::Sandbox;

pub(super) enum Live {
    Claimed(String),
    Expired(Timestamp),
    Nothing,
}

impl Sandbox {
    pub(super) async fn reuse_live(
        &self,
        turn: &TurnRef,
        parent: TurnId,
        seed_hash: &str,
        guard: &mut OpenGuard,
    ) -> Result<Live, Error> {
        let env = self.env();
        let keys = env.pod_keys();
        let selector = LabelSelector::new()
            .with(keys.pool(), POOL_CLAIMED)
            .with(keys.thread(), turn.thread.to_string())
            .with(keys.agent(), turn.agent.to_string());
        let mut pods = env.cluster.list_pods(&selector).await?;
        pods.sort_by(|left, right| left.name.cmp(&right.name));
        let policy = LivePolicy::of(&env.config);
        let now = env.clock.now();
        let mut expired = None;
        for pod in pods.into_iter().filter(|pod| !pod.deleting) {
            let marks = PodMarks::read(&pod.annotations, &keys);
            match judge_live_pod(&marks, parent, seed_hash, now, &policy) {
                LiveVerdict::Expired { expired_at } => {
                    self.remove_pod(&pod.name).await?;
                    expired.get_or_insert(expired_at);
                }
                LiveVerdict::Reusable if pod.is_ready() && self.claim_live(&pod, now).await? => {
                    guard.hold(pod.name.clone());
                    return Ok(Live::Claimed(pod.name));
                }
                _ => {}
            }
        }
        Ok(expired.map_or(Live::Nothing, Live::Expired))
    }

    async fn claim_live(&self, pod: &PodView, now: Timestamp) -> Result<bool, Error> {
        let patch = live_claim_patch(&self.env().pod_keys(), &pod.resource_version, now);
        match self.env().cluster.patch_pod(&pod.name, &patch).await {
            Ok(_) => Ok(true),
            Err(ClusterError::Conflict(_) | ClusterError::Gone(_)) => Ok(false),
            Err(other) => Err(other.into()),
        }
    }
}
