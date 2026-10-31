use std::time::{Duration, Instant};

use br_core_events::EventMetadata;
use br_core_integration::IntegrationCommand;
use br_core_kernel::Actor;
use br_util_nats_fabric::{CommandCoords, ConsumeErrorKind, FabricError, PublishErrorKind};
use contract_threads::{
    CompleteMessageV2, CreateThread, FailMessageV2, StoreMessageV2, ThreadsCommand,
};
use serde::Serialize;
use serde_json::Value;

use super::Live;
use super::answers::{self, outcome_coords};
use crate::backend::{Command, CommandBody, Outcome};
use crate::error::{Error, Failure};

struct Wire {
    coords: CommandCoords,
    command_type: String,
    version: u8,
    payload: Value,
}

fn wire<C: ThreadsCommand + Serialize>(payload: &C) -> Result<Wire, Error> {
    Ok(Wire {
        coords: C::coords(),
        command_type: C::command_type(),
        version: C::VERSION,
        payload: serde_json::to_value(payload).map_err(|e| Error::invalid(e.to_string()))?,
    })
}

fn wire_of(body: &CommandBody) -> Result<Wire, Error> {
    match body {
        CommandBody::CreateThread(payload) => wire::<CreateThread>(payload),
        CommandBody::Store(payload) => wire::<StoreMessageV2>(payload),
        CommandBody::Complete(payload) => wire::<CompleteMessageV2>(payload),
        CommandBody::Fail(payload) => wire::<FailMessageV2>(payload),
    }
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

impl Live {
    pub(crate) async fn run_command(
        &self,
        command: Command,
        wait: Duration,
    ) -> Result<Outcome, Error> {
        let deadline = Instant::now() + wait;
        let wire = wire_of(&command.body)?;
        let fabric = self.wire.fabric();
        if !self.wire.connected() {
            return Err(Error::unavailable("the bus connection is not connected"));
        }
        let events = outcome_coords();
        let refs: Vec<_> = events.iter().collect();
        let mut awaiter = tokio::time::timeout(wait, fabric.await_events(&refs))
            .await
            .map_err(|_| Error::Unavailable(Failure::wait("the answer subscription did not open")))?
            .map_err(|e| Error::unavailable(e.to_string()))?;
        let envelope = IntegrationCommand::new(
            command.id,
            wire.command_type,
            wire.version,
            chrono::Utc::now(),
            EventMetadata::new(Actor::Service(self.actor), command.id),
            wire.payload,
        );
        match tokio::time::timeout(
            remaining(deadline),
            fabric.publish_command(&wire.coords, &envelope),
        )
        .await
        {
            Err(_) => {
                return Err(Error::Lost(Failure::wait(
                    "the command was not acknowledged",
                )));
            }
            Ok(Err(FabricError::Publish {
                kind: PublishErrorKind::NoStream,
                detail,
            })) => return Err(Error::unavailable(detail)),
            Ok(Err(other)) => return Err(Error::lost(other.to_string())),
            Ok(Ok(())) => {}
        }
        let matched = awaiter
            .await_correlation(command.id, remaining(deadline))
            .await
            .map_err(|error| match error {
                FabricError::Consume {
                    kind: ConsumeErrorKind::ConsumerGone,
                    detail,
                } => Error::lost(detail),
                other => Error::lost(other.to_string()),
            })?
            .ok_or_else(|| Error::Lost(Failure::wait("no outcome within the wait")))?;
        answers::read(&matched.subject, &matched.payload)
    }
}
