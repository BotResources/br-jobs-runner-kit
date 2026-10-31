use std::sync::Arc;
use std::time::Duration;

use br_runner_connection::Connection;
use contract_jobs::runner::{ClaimRefusal, ClaimReplyV2};
use uuid::Uuid;

use crate::backend::{Backend, TriggerSource};
use crate::error::{Error, Operation, Refusal};
use crate::feed::Feed;
use crate::model::{CancelEvent, Capacity, ModeEvent, PresenceStatus};
use crate::work::{Unreadable, Work};

#[derive(Clone)]
pub struct JobsClient {
    backend: Arc<dyn Backend>,
}

impl std::fmt::Debug for JobsClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobsClient")
            .field("runner_type", &self.backend.runner_type())
            .field("instance_key", &self.backend.instance_key())
            .finish_non_exhaustive()
    }
}

pub struct TriggerFeed {
    backend: Arc<dyn Backend>,
    source: Box<dyn TriggerSource>,
}

impl std::fmt::Debug for TriggerFeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TriggerFeed").finish_non_exhaustive()
    }
}

impl TriggerFeed {
    pub async fn next(&mut self) -> Result<Work, Unreadable> {
        {
            let incoming = self.source.next().await.map_err(|error| Unreadable {
                error,
                job_id: None,
                run_id: None,
            })?;
            let value = match serde_json::from_slice(&incoming.raw) {
                Ok(value) => value,
                Err(e) => {
                    let _ = incoming.delivery.terminate().await;
                    return Err(Unreadable {
                        error: Error::Unreadable(format!("the trigger is not JSON: {e}")),
                        job_id: None,
                        run_id: None,
                    });
                }
            };
            let delivery = incoming.delivery;
            return match Work::from_value(Arc::clone(&self.backend), value, None) {
                Ok(work) => Ok(work.with_delivery(delivery)),
                Err(unreadable) => {
                    let _ = delivery.terminate().await;
                    Err(unreadable)
                }
            };
        }
    }
}

impl JobsClient {
    pub(crate) fn from_backend(backend: Arc<dyn Backend>) -> Self {
        Self { backend }
    }

    pub async fn new(
        runner_type: &str,
        instance_key: &str,
        reply_wait: Duration,
        connection: Connection,
    ) -> Result<Self, Error> {
        let live = crate::runner_transport::Live::connect(
            runner_type,
            instance_key,
            reply_wait,
            connection,
        )
        .await?;
        Ok(Self::from_backend(Arc::new(live)))
    }

    pub fn runner_type(&self) -> &str {
        self.backend.runner_type()
    }

    pub fn instance_key(&self) -> &str {
        self.backend.instance_key()
    }

    pub async fn dispatch_modes(&self) -> Result<Feed<ModeEvent>, Error> {
        Ok(Feed::new(self.backend.mode_feed().await?))
    }

    pub async fn cancel_orders(&self) -> Result<Feed<CancelEvent>, Error> {
        Ok(Feed::new(self.backend.cancel_feed().await?))
    }

    pub async fn triggers(&self, ack_wait: Duration) -> Result<TriggerFeed, Error> {
        if ack_wait.is_zero() {
            return Err(Error::invalid("the acknowledgement wait is zero"));
        }
        Ok(TriggerFeed {
            backend: Arc::clone(&self.backend),
            source: self.backend.open_triggers(ack_wait).await?,
        })
    }

    pub async fn claim(&self, claim_id: Uuid, wait: Duration) -> Result<Option<Work>, Error> {
        if wait.is_zero() {
            return Err(Error::invalid("the claim wait is zero"));
        }
        if claim_id.is_nil() {
            return Err(Error::invalid("the claim id is nil"));
        }
        let raw = self.backend.claim(claim_id, wait).await?;
        let value: serde_json::Value = serde_json::from_slice(&raw)
            .map_err(|e| Error::Unreadable(format!("the claim reply: {e}")))?;
        let run = value.get("run").filter(|run| !run.is_null()).cloned();
        let reply: ClaimReplyV2 = match &run {
            Some(_) => {
                let mut shell = value.clone();
                if let Some(object) = shell.as_object_mut() {
                    object.remove("run");
                }
                serde_json::from_value(shell)
                    .map_err(|e| Error::Unreadable(format!("the claim reply: {e}")))?
            }
            None => serde_json::from_value(value)
                .map_err(|e| Error::Unreadable(format!("the claim reply: {e}")))?,
        };
        if let Some(reason) = reply.reason {
            let word = match &reason {
                contract_jobs::reason::Reason::Known(known) => known.as_str().to_owned(),
                contract_jobs::reason::Reason::Unknown(text) => text.clone(),
            };
            let mut refusal = Refusal::new(Operation::Claim, word, String::new());
            if matches!(
                reason,
                contract_jobs::reason::Reason::Known(ClaimRefusal::Busy)
            ) {
                refusal.retry_after_ms = reply.retry_after_ms;
            }
            return Err(Error::Refused(refusal));
        }
        match run {
            None => Ok(None),
            Some(run) => Work::from_value(Arc::clone(&self.backend), run, None)
                .map(Some)
                .map_err(|unreadable| unreadable.error),
        }
    }

    pub async fn publish_presence(
        &self,
        runner_version: &str,
        status: PresenceStatus,
        capacity: Capacity,
    ) -> Result<(), Error> {
        self.backend
            .presence_put(runner_version.to_owned(), status, capacity.get())
            .await
    }

    pub async fn delete_presence(&self) -> Result<(), Error> {
        self.backend.presence_delete().await
    }

    pub async fn presence_time_to_live(&self) -> Result<Duration, Error> {
        self.backend.presence_ttl().await
    }
}
