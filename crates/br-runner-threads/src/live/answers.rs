use br_util_nats_fabric::{EventCoords, event_subject};
use contract_threads::{
    MessageCommandConfirmed, MessageCommandRefused, ReasonCode, ThreadCommandConfirmed,
    ThreadCommandRefused, ThreadsEvent,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::backend::Outcome;
use crate::error::{Error, Refusal};
use crate::message::MessageStatus;

pub(crate) fn outcome_coords() -> [EventCoords; 4] {
    [
        ThreadCommandConfirmed::coords(),
        ThreadCommandRefused::coords(),
        MessageCommandConfirmed::coords(),
        MessageCommandRefused::coords(),
    ]
}

#[derive(Deserialize)]
struct Envelope {
    payload: Value,
}

fn unreadable(error: serde_json::Error) -> Error {
    Error::Unreadable(format!("an answer cannot be read: {error}"))
}

fn status_of(status: contract_threads::MessageStatus) -> MessageStatus {
    match status {
        contract_threads::MessageStatus::Streaming => MessageStatus::Streaming,
        contract_threads::MessageStatus::Completed => MessageStatus::Completed,
        contract_threads::MessageStatus::Failed => MessageStatus::Failed,
        contract_threads::MessageStatus::Cancelled => MessageStatus::Cancelled,
    }
}

fn refused(command_id: Uuid, code: ReasonCode, params: Map<String, Value>) -> Error {
    Error::Refused(Refusal {
        code: code.as_str().to_owned(),
        reason: None,
        params,
        message: String::new(),
        command_id: Some(command_id),
    })
}

pub(crate) fn read(subject: &str, bytes: &[u8]) -> Result<Outcome, Error> {
    let Envelope { payload } = serde_json::from_slice(bytes).map_err(unreadable)?;
    if subject == event_subject(&ThreadCommandConfirmed::coords()) {
        let event: ThreadCommandConfirmed = serde_json::from_value(payload).map_err(unreadable)?;
        return Ok(Outcome {
            command_id: event.command_id,
            status: None,
        });
    }
    if subject == event_subject(&ThreadCommandRefused::coords()) {
        let event: ThreadCommandRefused = serde_json::from_value(payload).map_err(unreadable)?;
        return Err(refused(event.command_id, event.reason_code, event.params));
    }
    if subject == event_subject(&MessageCommandConfirmed::coords()) {
        let event: MessageCommandConfirmed = serde_json::from_value(payload).map_err(unreadable)?;
        return Ok(Outcome {
            command_id: event.command_id,
            status: Some(status_of(event.status)),
        });
    }
    if subject == event_subject(&MessageCommandRefused::coords()) {
        let event: MessageCommandRefused = serde_json::from_value(payload).map_err(unreadable)?;
        return Err(refused(event.command_id, event.reason_code, event.params));
    }
    Err(Error::Unreadable(
        "an answer came on a subject that is none of the four outcomes".to_owned(),
    ))
}
