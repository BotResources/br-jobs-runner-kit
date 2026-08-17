use std::time::Duration;

use async_nats::jetstream::kv::{Operation, Store, Watch};
use br_jobs_runner_core::Backoff;
use futures::StreamExt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelEvent {
    Cancelled(Uuid),
    Cleared(Uuid),
    Reset,
}

pub(crate) struct CancelFeed {
    store: Store,
    watch: Option<Watch>,
}

impl CancelFeed {
    pub(crate) fn new(store: Store) -> Self {
        Self { store, watch: None }
    }

    pub(crate) async fn open(&mut self) -> Result<(), crate::error::HarnessError> {
        let watch = self.store.watch_with_history(">").await.map_err(|error| {
            crate::error::HarnessError::infra("opening the cancel watch", error)
        })?;
        self.watch = Some(watch);
        Ok(())
    }

    pub(crate) async fn next(&mut self) -> CancelEvent {
        let mut backoff = reconnect_backoff();
        loop {
            if let Some(watch) = self.watch.as_mut() {
                match watch.next().await {
                    Some(Ok(entry)) => {
                        let Ok(run_id) = entry.key.parse::<Uuid>() else {
                            tracing::warn!(key = %entry.key, "ignoring a non-run cancel key");
                            continue;
                        };
                        return match entry.operation {
                            Operation::Put => CancelEvent::Cancelled(run_id),
                            Operation::Delete | Operation::Purge => CancelEvent::Cleared(run_id),
                        };
                    }
                    Some(Err(error)) => {
                        tracing::warn!(%error, "cancel watch errored; replaying the bucket");
                    }
                    None => {
                        tracing::warn!("cancel watch ended; replaying the bucket");
                    }
                }
                self.watch = None;
            }
            tokio::time::sleep(backoff.next_delay()).await;
            match self.store.watch_with_history(">").await {
                Ok(watch) => {
                    self.watch = Some(watch);
                    backoff.reset();
                    return CancelEvent::Reset;
                }
                Err(error) => {
                    tracing::warn!(%error, "reopening the cancel watch failed");
                }
            }
        }
    }
}

fn reconnect_backoff() -> Backoff {
    Backoff::new(Duration::from_millis(500), Duration::from_secs(15))
        .expect("the reconnect backoff bounds are static and valid")
}
