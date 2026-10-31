use std::time::Duration;

use async_nats::jetstream::AckKind;
use async_nats::jetstream::Message;
use async_nats::jetstream::consumer::pull::{Config, MessagesErrorKind, Stream as Pulled};
use async_nats::jetstream::consumer::{AckPolicy, DeliverPolicy};
use contract_jobs::runner::TRIGGER_STREAM;
use contract_jobs::runner_transport::trigger_v2_subject;
use futures::StreamExt;

use super::{Live, fail};
use crate::backend::{BoxFuture, IncomingTrigger, TriggerDelivery, TriggerSource};
use crate::error::Error;

pub(super) const DURABLE_PREFIX: &str = "runner_jobs_trigger_";

struct Delivery(Message);

impl Delivery {
    fn settle(&self, kind: AckKind) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.0
                .double_ack_with(kind)
                .await
                .map_err(|e| fail::sent("the trigger stream", e.as_ref()))
        })
    }
}

impl TriggerDelivery for Delivery {
    fn ack(&self) -> BoxFuture<'_, Result<(), Error>> {
        self.settle(AckKind::Ack)
    }

    fn give_back(&self) -> BoxFuture<'_, Result<(), Error>> {
        self.settle(AckKind::Nak(None))
    }

    fn terminate(&self) -> BoxFuture<'_, Result<(), Error>> {
        self.settle(AckKind::Term)
    }
}

struct Triggers {
    messages: Pulled,
    jetstream: async_nats::jetstream::Context,
}

impl Triggers {
    async fn failure(&self, kind: MessagesErrorKind, text: String) -> Error {
        if self.jetstream.get_stream(TRIGGER_STREAM).await.is_err() {
            return Error::InfraMissing(format!("the stream {TRIGGER_STREAM} is missing"));
        }
        match kind {
            MessagesErrorKind::MissingHeartbeat => {
                Error::lost(format!("the trigger consumer missed its heartbeat: {text}"))
            }
            _ => Error::lost(format!("the trigger consumer failed: {text}")),
        }
    }
}

impl TriggerSource for Triggers {
    fn next(&mut self) -> BoxFuture<'_, Result<IncomingTrigger, Error>> {
        Box::pin(async move {
            match self.messages.next().await {
                Some(Ok(message)) => Ok(IncomingTrigger {
                    raw: message.payload.to_vec(),
                    delivery: Box::new(Delivery(message)),
                }),
                Some(Err(error)) => Err(self.failure(error.kind(), error.to_string()).await),
                None => Err(Error::lost("the trigger consumer ended")),
            }
        })
    }
}

impl Live {
    fn trigger_durable(&self) -> String {
        format!("{DURABLE_PREFIX}{}", self.segments.runner_type.as_str())
    }

    pub(super) async fn trigger_source(
        &self,
        ack_wait: Duration,
    ) -> Result<Box<dyn TriggerSource>, Error> {
        let object = format!("the stream {TRIGGER_STREAM}");
        let stream = self
            .wire
            .jetstream
            .get_stream(TRIGGER_STREAM)
            .await
            .map_err(|e| fail::unsent(&object, &e))?;
        let consumer = stream
            .create_consumer(Config {
                durable_name: Some(self.trigger_durable()),
                filter_subject: trigger_v2_subject(&self.segments.runner_type),
                ack_policy: AckPolicy::Explicit,
                ack_wait,
                deliver_policy: DeliverPolicy::All,
                max_deliver: -1,
                max_ack_pending: -1,
                ..Config::default()
            })
            .await
            .map_err(|e| fail::unsent("the trigger consumer", &e))?;
        let messages = consumer
            .stream()
            .max_messages_per_batch(1)
            .messages()
            .await
            .map_err(|e| fail::unsent("the trigger consumer", &e))?;
        Ok(Box::new(Triggers {
            messages,
            jetstream: self.wire.jetstream.clone(),
        }))
    }
}
