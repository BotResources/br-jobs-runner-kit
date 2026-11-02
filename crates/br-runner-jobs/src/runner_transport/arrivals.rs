use async_nats::jetstream::Context;
use async_nats::jetstream::consumer::pull::{Config, MessagesError, MessagesErrorKind, Stream};
use async_nats::jetstream::consumer::{AckPolicy, DeliverPolicy};
use br_util_nats_fabric::{INTEGRATION_EVT, IntegrationEvent, event_subject};
use contract_jobs::event_v2::{ItemKind as WireKind, RunItemArrivedV2};
use futures::StreamExt;
use uuid::Uuid;

use super::{Live, fail};
use crate::arrival::ArrivalSettings;
use crate::backend::{BoxFuture, RawFeed};
use crate::error::Error;
use crate::model::{ItemArrived, ItemKind};

fn kind_of(kind: WireKind) -> ItemKind {
    match kind {
        WireKind::Mail => ItemKind::DirectMail,
        WireKind::CcNotice => ItemKind::CcNotice,
        WireKind::Notice => ItemKind::CallNotice,
        WireKind::MailNotice => ItemKind::MailNotice,
    }
}

pub(super) struct Delivered {
    pub(super) payload: Vec<u8>,
    pub(super) subject: String,
}

pub(super) type Pulled = Option<Result<Delivered, MessagesError>>;

pub(super) trait Pull: Send + Sync {
    fn pull(&mut self) -> BoxFuture<'_, Pulled>;
}

impl Pull for Stream {
    fn pull(&mut self) -> BoxFuture<'_, Pulled> {
        Box::pin(async move {
            let next = self.next().await?;
            Some(match next {
                Ok(message) => {
                    message.ack().await.ok();
                    Ok(Delivered {
                        payload: message.payload.to_vec(),
                        subject: message.subject.to_string(),
                    })
                }
                Err(error) => Err(error),
            })
        })
    }
}

pub(super) struct Doorbell {
    run_id: Uuid,
    messages: Box<dyn Pull>,
    jetstream: Context,
    consumer: String,
    ended: Option<Error>,
}

impl Doorbell {
    pub(super) fn new(
        run_id: Uuid,
        messages: Box<dyn Pull>,
        jetstream: Context,
        consumer: String,
    ) -> Self {
        Self {
            run_id,
            messages,
            jetstream,
            consumer,
            ended: None,
        }
    }

    async fn check(&self) -> Result<(), Error> {
        let object = format!("the stream {INTEGRATION_EVT}");
        let stream = self
            .jetstream
            .get_stream(INTEGRATION_EVT)
            .await
            .map_err(|e| match fail::unsent(&object, &e) {
                missing @ Error::InfraMissing(_) => missing,
                other => Error::lost(format!("the arrival consumer cannot be checked: {other}")),
            })?;
        stream
            .consumer_info(&self.consumer)
            .await
            .map(drop)
            .map_err(|e| Error::lost(format!("the arrival consumer is gone: {e}")))
    }

    async fn pull_failed(&self, error: MessagesError) -> Error {
        match self.check().await {
            Err(gone) => gone,
            Ok(()) => Error::lost(format!("the arrival consumer failed: {error}")),
        }
    }

    fn arrived(&self, payload: &[u8], subject: &str) -> Option<Result<ItemArrived, Error>> {
        let event: IntegrationEvent<RunItemArrivedV2> = match serde_json::from_slice(payload) {
            Ok(event) => event,
            Err(_) => return Some(Err(Error::unreadable_payload(subject))),
        };
        let arrival = event.payload;
        (arrival.run_id == self.run_id).then_some(Ok(ItemArrived {
            address: arrival.address,
            item_id: arrival.item_id,
            kind: kind_of(arrival.item_kind),
        }))
    }

    async fn next_arrival(&mut self) -> Result<ItemArrived, Error> {
        loop {
            if let Some(error) = &self.ended {
                return Err(error.clone());
            }
            let failure = match self.messages.pull().await {
                Some(Ok(message)) => match self.arrived(&message.payload, &message.subject) {
                    Some(found) => return found,
                    None => continue,
                },
                Some(Err(error)) if error.kind() == MessagesErrorKind::MissingHeartbeat => {
                    match self.check().await {
                        Ok(()) => continue,
                        Err(gone) => gone,
                    }
                }
                Some(Err(error)) => self.pull_failed(error).await,
                None => match self.check().await {
                    Err(gone) => gone,
                    Ok(()) => Error::lost("the arrival consumer ended"),
                },
            };
            self.ended = Some(failure);
        }
    }
}

impl RawFeed<ItemArrived> for Doorbell {
    fn next(&mut self) -> BoxFuture<'_, Result<ItemArrived, Error>> {
        Box::pin(self.next_arrival())
    }
}

impl Live {
    pub(super) async fn open_arrival_feed(
        &self,
        run_id: Uuid,
        settings: ArrivalSettings,
    ) -> Result<Box<dyn RawFeed<ItemArrived>>, Error> {
        let coords = contract_jobs::evt_run_item_arrived_v2_coords()
            .map_err(|e| Error::invalid(e.to_string()))?;
        let object = format!("the stream {INTEGRATION_EVT}");
        let stream = self
            .wire
            .jetstream
            .get_stream(INTEGRATION_EVT)
            .await
            .map_err(|e| fail::unsent(&object, &e))?;
        let consumer = stream
            .create_consumer(Config {
                filter_subject: event_subject(&coords),
                deliver_policy: DeliverPolicy::New,
                ack_policy: AckPolicy::Explicit,
                ack_wait: settings.ack_wait,
                inactive_threshold: settings.inactivity_limit,
                ..Config::default()
            })
            .await
            .map_err(|e| fail::unsent("the arrival consumer", &e))?;
        let name = consumer.cached_info().name.clone();
        let messages = consumer
            .messages()
            .await
            .map_err(|e| fail::unsent("the arrival consumer", &e))?;
        Ok(Box::new(Doorbell::new(
            run_id,
            Box::new(messages),
            self.wire.jetstream.clone(),
            name,
        )))
    }
}
