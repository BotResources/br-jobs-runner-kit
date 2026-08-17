use async_nats::jetstream::kv::Store;
use br_jobs_runner_core::PresenceAction;
use contract_jobs::runner::{Presence, RunnerStatus, WIRE_VERSION};
use contract_jobs::runner_transport::runner_presence_key;

use crate::config::RunnerConfig;

pub(crate) struct PresencePublisher {
    store: Store,
    key: String,
    ready: Vec<u8>,
    draining: Vec<u8>,
}

impl PresencePublisher {
    pub(crate) fn new(store: Store, config: &RunnerConfig) -> Self {
        let key = runner_presence_key(config.runner_type.segment(), config.instance_key.segment());
        Self {
            store,
            key,
            ready: entry(config, RunnerStatus::Ready),
            draining: entry(config, RunnerStatus::Draining),
        }
    }

    pub(crate) async fn apply(&self, action: PresenceAction) {
        let error = match action {
            PresenceAction::PublishReady => self
                .store
                .put(&self.key, self.ready.clone().into())
                .await
                .err()
                .map(|error| error.to_string()),
            PresenceAction::PublishDraining => self
                .store
                .put(&self.key, self.draining.clone().into())
                .await
                .err()
                .map(|error| error.to_string()),
            PresenceAction::Delete => self
                .store
                .delete(&self.key)
                .await
                .err()
                .map(|error| error.to_string()),
        };
        if let Some(error) = error {
            tracing::warn!(key = %self.key, error, "presence write failed; the next heartbeat retries");
        }
    }
}

fn entry(config: &RunnerConfig, status: RunnerStatus) -> Vec<u8> {
    serde_json::to_vec(&Presence {
        version: WIRE_VERSION,
        runner_type: config.runner_type.as_str().to_owned(),
        instance_key: config.instance_key.as_str().to_owned(),
        runner_version: config.runner_version.clone(),
        status,
        capacity: config.capacity(),
    })
    .expect("a presence entry always serializes")
}
