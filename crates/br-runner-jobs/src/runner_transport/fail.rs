use std::error::Error as StdError;

use async_nats::jetstream::ErrorCode;
use async_nats::jetstream::context::{
    GetStreamError, GetStreamErrorKind, PublishError, PublishErrorKind, RequestError,
    RequestErrorKind,
};
use async_nats::jetstream::stream::{ConsumerError, ConsumerErrorKind};
use br_util_nats_fabric::{ConsumeErrorKind, FabricError, PublishErrorKind as FabricPublish};

use crate::error::{Error, Failure};

const STREAM_NOT_FOUND_TEXT: &str = "stream not found";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    Missing,
    Timeout,
    Other,
}

fn single(error: &(dyn StdError + 'static)) -> Class {
    if let Some(e) = error.downcast_ref::<PublishError>() {
        match e.kind() {
            PublishErrorKind::StreamNotFound => return Class::Missing,
            PublishErrorKind::TimedOut => return Class::Timeout,
            _ => {}
        }
    }
    if let Some(e) = error.downcast_ref::<GetStreamError>()
        && let GetStreamErrorKind::JetStream(js) = e.kind()
        && js.error_code() == ErrorCode::STREAM_NOT_FOUND
    {
        return Class::Missing;
    }
    if let Some(e) = error.downcast_ref::<async_nats::jetstream::Error>()
        && e.error_code() == ErrorCode::STREAM_NOT_FOUND
    {
        return Class::Missing;
    }
    if let Some(e) = error.downcast_ref::<ConsumerError>()
        && let ConsumerErrorKind::JetStream(js) = e.kind()
        && js.error_code() == ErrorCode::STREAM_NOT_FOUND
    {
        return Class::Missing;
    }
    if let Some(e) = error.downcast_ref::<RequestError>()
        && e.kind() == RequestErrorKind::TimedOut
    {
        return Class::Timeout;
    }
    if let Some(e) = error.downcast_ref::<async_nats::client::RequestError>()
        && e.kind() == async_nats::client::RequestErrorKind::TimedOut
    {
        return Class::Timeout;
    }
    if let Some(e) = error.downcast_ref::<std::io::Error>()
        && e.kind() == std::io::ErrorKind::TimedOut
    {
        return Class::Timeout;
    }
    Class::Other
}

pub(super) fn classify(error: &(dyn StdError + 'static)) -> Class {
    let mut current: Option<&(dyn StdError + 'static)> = Some(error);
    while let Some(e) = current {
        match single(e) {
            Class::Other => current = e.source(),
            found => return found,
        }
    }
    Class::Other
}

fn text(error: &(dyn StdError + 'static)) -> String {
    let mut text = error.to_string();
    let mut current = error.source();
    while let Some(cause) = current {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        current = cause.source();
    }
    text
}

pub(super) fn missing(object: &str) -> Error {
    Error::InfraMissing(format!("{object} is missing"))
}

pub(super) fn unsent(object: &str, error: &(dyn StdError + 'static)) -> Error {
    match classify(error) {
        Class::Missing => missing(object),
        Class::Timeout => Error::Unavailable(Failure::wait(format!("{object}: {}", text(error)))),
        Class::Other => Error::unavailable(format!("{object}: {}", text(error))),
    }
}

pub(super) fn sent(object: &str, error: &(dyn StdError + 'static)) -> Error {
    match classify(error) {
        Class::Missing => missing(object),
        Class::Timeout => Error::wait_fired(format!("{object}: {}", text(error))),
        Class::Other => Error::lost(format!("{object}: {}", text(error))),
    }
}

pub(super) fn fabric(object: &str, error: FabricError) -> Error {
    match error {
        FabricError::Consume {
            kind: ConsumeErrorKind::NoStream,
            ..
        }
        | FabricError::Publish {
            kind: FabricPublish::NoStream,
            ..
        }
        | FabricError::SubjectNotCovered { .. } => missing(object),
        FabricError::Kv(detail) if detail.contains(STREAM_NOT_FOUND_TEXT) => missing(object),
        FabricError::Publish {
            kind: FabricPublish::Timeout,
            detail,
        } => Error::wait_fired(format!("{object}: {detail}")),
        FabricError::Decode { subject, detail } => {
            Error::Unreadable(format!("{subject}: {detail}"))
        }
        other => Error::lost(format!("{object}: {other}")),
    }
}

pub(super) fn fabric_unsent(object: &str, error: FabricError) -> Error {
    match fabric(object, error) {
        Error::Lost(failure) => Error::Unavailable(failure),
        other => other,
    }
}
