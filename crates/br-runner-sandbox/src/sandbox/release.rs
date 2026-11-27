use br_agentic_harness_contract::BoxId;

use crate::cluster::ClusterError;
use crate::decide::{ReleasePlan, idle_patch, release_plan};
use crate::error::Error;

use super::state::Sandbox;

impl Sandbox {
    pub async fn release_box(&self, id: &BoxId) -> Result<(), Error> {
        let saved = {
            let mut state = self.inner.state();
            let record = state
                .boxes
                .get_mut(id.as_str())
                .ok_or_else(|| Error::UnknownBox(id.to_string()))?;
            if record.released {
                return Ok(());
            }
            record.released = true;
            let (instance, saved) = (record.instance, record.saved_bytes);
            state.instances.remove(&instance);
            saved
        };
        let env = self.env();
        let outcome = match release_plan(saved, env.config.idle_keep_min_state_bytes()) {
            ReleasePlan::Delete => env.cluster.delete_pod(id.as_str()).await,
            ReleasePlan::KeepIdle => env
                .cluster
                .patch_pod(id.as_str(), &idle_patch(&env.pod_keys(), env.clock.now()))
                .await
                .map(|_| ()),
        };
        match outcome {
            Ok(()) | Err(ClusterError::Gone(_)) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
