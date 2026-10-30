use std::sync::Arc;

use async_nats::jetstream;
use br_core_events::EventMetadata;
use br_core_integration::IntegrationCommand;
use br_core_kernel::Actor;
use br_util_nats_fabric::{
    ConnectionState, Fabric, FabricError, INTEGRATION_CMD, INTEGRATION_EVT, PublishErrorKind,
};
use contract_threads::coords::versioned_command_coords;
use contract_threads::{ChunkFrame, STREAMING_STREAM};

use super::nats_answers::Answers;
use super::{AnswerWait, BoxFuture, BusCommand, BusTransport, OutboundChunk};
use crate::error::Error;

#[derive(Clone)]
pub struct NatsBus {
    fabric: Fabric,
    jetstream: jetstream::Context,
    answers: Arc<Answers>,
}

impl NatsBus {
    pub async fn connect(client: async_nats::Client) -> Result<Self, Error> {
        let jetstream = jetstream::new(client.clone());
        for stream in [INTEGRATION_CMD, INTEGRATION_EVT, STREAMING_STREAM] {
            jetstream
                .get_stream(stream)
                .await
                .map_err(|e| Error::Infra(format!("the stream {stream} cannot be bound: {e}")))?;
        }
        Ok(Self {
            fabric: Fabric::new(jetstream.clone()),
            jetstream,
            answers: Answers::new(client),
        })
    }
}

impl BusTransport for NatsBus {
    fn ready(&self) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            if self.fabric.connection_state() != ConnectionState::Connected {
                return Err(Error::Unavailable(
                    "the connection is not connected".to_owned(),
                ));
            }
            self.answers.ensure_open().await
        })
    }

    fn send(&self, command: BusCommand) -> BoxFuture<'_, Result<AnswerWait, Error>> {
        Box::pin(async move {
            let waiter = self.answers.register(command.command_id)?;
            let coords = versioned_command_coords(command.aggregate, command.verb, command.version);
            let envelope = IntegrationCommand::new(
                command.command_id,
                format!("{}.{}", command.aggregate, command.verb),
                command.version,
                chrono::Utc::now(),
                EventMetadata::new(Actor::Service(command.actor), command.command_id),
                command.payload,
            );
            match self.fabric.publish_command(&coords, &envelope).await {
                Ok(()) => Ok(waiter.into_wait()),
                Err(FabricError::Publish {
                    kind: PublishErrorKind::NoStream,
                    detail,
                }) => Err(Error::Unavailable(detail)),
                Err(other) => Err(Error::AnswerLost(other.to_string())),
            }
        })
    }

    fn publish_chunk(&self, chunk: OutboundChunk) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            let frame: ChunkFrame =
                serde_json::from_value(chunk.frame).map_err(|_| Error::StreamBroken)?;
            let bytes = serde_json::to_vec(&frame).map_err(|_| Error::StreamBroken)?;
            let mut headers = async_nats::HeaderMap::new();
            headers.insert(async_nats::header::NATS_MESSAGE_ID, frame.msg_id());
            let ack = self
                .jetstream
                .publish_with_headers(frame.subject(), headers, bytes.into())
                .await
                .map_err(|_| Error::StreamBroken)?;
            ack.await.map_err(|_| Error::StreamBroken)?;
            Ok(())
        })
    }
}
