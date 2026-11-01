use std::sync::Arc;
use std::time::Duration;

use br_runner_jobs::{Capacity, Error, JobsClient, LogLevel, PresenceStatus, TriggerFeed, Work};
use uuid::Uuid;

use crate::tap::{HostEvent, Tap, tell};

#[derive(Clone)]
pub(crate) struct Calls {
    client: JobsClient,
    tap: Option<Arc<Tap>>,
}

impl Calls {
    pub(crate) fn new(client: JobsClient, tap: Option<Arc<Tap>>) -> Self {
        Self { client, tap }
    }

    pub(crate) fn client(&self) -> &JobsClient {
        &self.client
    }

    pub(crate) fn tap(&self) -> &Option<Arc<Tap>> {
        &self.tap
    }

    pub(crate) async fn claim(
        &self,
        claim_id: Uuid,
        wait: Duration,
    ) -> Result<Option<Work>, Error> {
        tell(&self.tap, || HostEvent::Claim { claim_id });
        self.client.claim(claim_id, wait).await
    }

    pub(crate) async fn triggers(&self, ack_wait: Duration) -> Result<TriggerFeed, Error> {
        self.client.triggers(ack_wait).await
    }

    pub(crate) fn acknowledge(&self, work: Arc<Work>) {
        tell(&self.tap, || HostEvent::TriggerAcknowledged);
        tokio::spawn(async move {
            if let Err(error) = work.ack().await {
                tracing::error!(%error, "the trigger could not be acknowledged");
            }
        });
    }

    pub(crate) fn give_back(&self, work: Work) {
        tell(&self.tap, || HostEvent::TriggerGivenBack);
        tokio::spawn(async move {
            if let Err(error) = work.give_back().await {
                tracing::error!(%error, "the trigger could not be given back");
            }
        });
    }

    pub(crate) async fn publish_presence(
        &self,
        runner_version: &str,
        status: PresenceStatus,
        capacity: Capacity,
    ) -> Result<(), Error> {
        tell(&self.tap, || HostEvent::PresencePublished {
            runner_version: runner_version.to_owned(),
            status,
            capacity: capacity.get(),
        });
        self.client
            .publish_presence(runner_version, status, capacity)
            .await
    }

    pub(crate) async fn delete_presence(&self) -> Result<(), Error> {
        tell(&self.tap, || HostEvent::PresenceDeleted);
        self.client.delete_presence().await
    }

    pub(crate) async fn run_log(
        &self,
        work: &Work,
        level: LogLevel,
        message: String,
        step_index: Option<u32>,
    ) -> Result<(), Error> {
        tell(&self.tap, || HostEvent::RunLog {
            level,
            message: message.clone(),
        });
        work.log(level, message, step_index, None).await
    }
}
