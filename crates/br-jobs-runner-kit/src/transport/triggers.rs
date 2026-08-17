use std::time::Duration;

use async_nats::jetstream::Context;
use async_nats::jetstream::consumer::pull::{Config, Stream as MessageStream};
use async_nats::jetstream::consumer::{AckPolicy, PullConsumer};
use br_jobs_runner_core::Backoff;
use contract_jobs::runner::TRIGGER_STREAM;
use contract_jobs::runner_transport::trigger_subject;
use contract_jobs::segment::SubjectSegment;

use crate::error::HarnessError;

pub(crate) struct TriggerFeed {
    js: Context,
    durable: String,
    filter: String,
    ack_wait: Duration,
    messages: Option<MessageStream>,
}

impl TriggerFeed {
    pub(crate) fn new(js: Context, runner_type: &SubjectSegment, ack_wait: Duration) -> Self {
        Self {
            js,
            durable: format!("runner_{}", runner_type.as_str()),
            filter: trigger_subject(runner_type),
            ack_wait,
            messages: None,
        }
    }

    pub(crate) async fn connect(&mut self) -> Result<(), HarnessError> {
        let consumer = self.consumer().await?;
        let messages = consumer
            .stream()
            .max_messages_per_batch(1)
            .messages()
            .await
            .map_err(|error| HarnessError::infra("opening the trigger pull stream", error))?;
        self.messages = Some(messages);
        Ok(())
    }

    pub(crate) async fn next(&mut self) -> async_nats::jetstream::Message {
        let mut backoff = reconnect_backoff();
        loop {
            if let Some(messages) = self.messages.as_mut() {
                match futures::StreamExt::next(messages).await {
                    Some(Ok(message)) => return message,
                    Some(Err(error)) => {
                        tracing::warn!(%error, "trigger stream errored; reconnecting");
                    }
                    None => {
                        tracing::warn!("trigger stream ended; reconnecting");
                    }
                }
                self.messages = None;
            }
            tokio::time::sleep(backoff.next_delay()).await;
            match self.connect().await {
                Ok(()) => backoff.reset(),
                Err(error) => tracing::warn!(%error, "reattaching the trigger consumer failed"),
            }
        }
    }

    async fn consumer(&self) -> Result<PullConsumer, HarnessError> {
        let stream =
            self.js.get_stream(TRIGGER_STREAM).await.map_err(|error| {
                HarnessError::infra("binding the declared trigger stream", error)
            })?;
        stream
            .create_consumer(Config {
                durable_name: Some(self.durable.clone()),
                filter_subject: self.filter.clone(),
                ack_policy: AckPolicy::Explicit,
                ack_wait: self.ack_wait,
                max_ack_pending: 256,
                max_deliver: -1,
                ..Default::default()
            })
            .await
            .map_err(|error| {
                HarnessError::infra(
                    "creating the shared trigger durable",
                    format!("{}: {error}", self.durable),
                )
            })
    }
}

fn reconnect_backoff() -> Backoff {
    Backoff::new(Duration::from_millis(500), Duration::from_secs(15))
        .expect("the reconnect backoff bounds are static and valid")
}
