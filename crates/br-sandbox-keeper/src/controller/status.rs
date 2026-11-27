use crate::pool::{
    Acceptance, Observation, StatusInput, StatusWrite, compute_status, decide_write,
};
use crate::resource::SandboxPool;

use super::Engine;

impl Engine {
    pub(super) async fn write_status(
        &mut self,
        namespace: &str,
        pool: &SandboxPool,
        acceptance: &Acceptance,
        observation: Option<&Observation>,
        min_replicas: Option<u32>,
    ) {
        let now = self.clock.now();
        let name = pool.metadata.name.clone().unwrap_or_default();
        let input = StatusInput::new(
            pool.metadata.generation,
            min_replicas,
            acceptance,
            observation,
            pool.status.as_ref(),
            now,
        );
        let key = (namespace.to_owned(), name.clone());
        let last = self.status_written.get(&key).copied();
        match decide_write(compute_status(&input), pool.status.as_ref(), last, now) {
            StatusWrite::Skip => {}
            StatusWrite::Wait { until } => self.wake_by(namespace, until),
            StatusWrite::Write(status) => {
                if !self.leadership.is_leader() {
                    return;
                }
                match self.writer.patch_status(namespace, &name, &status).await {
                    Ok(()) => {
                        self.status_written.insert(key, now);
                    }
                    Err(error) => {
                        tracing::warn!(pool = namespace, %error, "the status will be written again");
                        self.retry_soon(namespace);
                    }
                }
            }
        }
    }
}
