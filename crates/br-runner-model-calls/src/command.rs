use br_util_nats_fabric::{CommandCoords, IntegrationCommand};
use serde::Serialize;

use crate::record::ModelCallRecord;

/// The seam through which the host gives the command of the consuming service.
///
/// The host implements it once for a consuming contract: the contract crate of
/// that service owns the subject and the payload, and this library depends on
/// no such contract.
pub trait RecordCommand: Send + Sync + 'static {
    /// The payload of the command, as the contract of the consuming service
    /// defines it.
    type Payload: Serialize + Send + Sync;

    /// The coordinates of the command. The library publishes on their subject.
    fn coords(&self) -> &CommandCoords;

    /// The command published for one record. The host maps the record to its
    /// payload and fills the envelope (type, version, issue time, metadata). The
    /// host sets the command id to [`ModelCallRecord::call_id`].
    ///
    /// The library calls it once per record, in a task of its own, never during
    /// the model call.
    fn command(&self, record: &ModelCallRecord) -> IntegrationCommand<Self::Payload>;
}

/// The command with its payload turned into JSON once, so that every attempt
/// publishes the same bytes and a payload that cannot be serialized is never
/// sent again.
pub(crate) trait ErasedCommand: Send + Sync + 'static {
    fn coords(&self) -> &CommandCoords;

    fn envelope(
        &self,
        record: &ModelCallRecord,
    ) -> Result<IntegrationCommand<serde_json::Value>, serde_json::Error>;
}

impl<R: RecordCommand> ErasedCommand for R {
    fn coords(&self) -> &CommandCoords {
        RecordCommand::coords(self)
    }

    fn envelope(
        &self,
        record: &ModelCallRecord,
    ) -> Result<IntegrationCommand<serde_json::Value>, serde_json::Error> {
        let command = self.command(record);
        let payload = serde_json::to_value(&command.payload)?;
        Ok(IntegrationCommand::new(
            command.command_id,
            command.command_type,
            command.version,
            command.issued_at,
            command.metadata,
            payload,
        ))
    }
}
