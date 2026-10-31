use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Cause {
    WaitFired,
    Transport,
    NoListener,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Failure {
    pub cause: Cause,
    pub text: String,
}

impl Failure {
    pub fn new(cause: Cause, text: impl Into<String>) -> Self {
        Self {
            cause,
            text: text.into(),
        }
    }

    pub(crate) fn wait(text: impl Into<String>) -> Self {
        Self::new(Cause::WaitFired, text)
    }

    pub(crate) fn transport(text: impl Into<String>) -> Self {
        Self::new(Cause::Transport, text)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cause = match self.cause {
            Cause::WaitFired => "the wait fired",
            Cause::Transport => "the transport failed",
            Cause::NoListener => "no listener",
        };
        write!(f, "{cause}: {}", self.text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CredentialKind {
    NoUsable,
    FetchFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
#[error("{}: {text}", match .kind {
    CredentialKind::NoUsable => "no usable credential",
    CredentialKind::FetchFailed => "credential fetch failed",
})]
pub struct CredentialFailure {
    pub kind: CredentialKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Refusal {
    pub code: String,
    pub reason: Option<String>,
    pub params: Map<String, Value>,
    pub message: String,
    pub command_id: Option<Uuid>,
}

impl Refusal {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        command_id: Option<Uuid>,
    ) -> Self {
        Self {
            code: code.into(),
            reason: None,
            params: Map::new(),
            message: message.into(),
            command_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("{0}")]
    Credential(CredentialFailure),
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("unavailable, {0}")]
    Unavailable(Failure),
    #[error("lost, {0}")]
    Lost(Failure),
    #[error("refused: {}", .0.code)]
    Refused(Refusal),
    #[error("unreadable: {0}")]
    Unreadable(String),
    #[error("stream broken")]
    StreamBroken,
    #[error("stream closed")]
    StreamClosed,
    #[error("infra missing: {0}")]
    InfraMissing(String),
    #[error("store: {0}")]
    Store(String),
}

impl Error {
    pub(crate) fn invalid(reason: impl Into<String>) -> Self {
        Self::InvalidInput(reason.into())
    }

    pub(crate) fn library(error: &br_llm_messages::Error) -> Self {
        Self::InvalidInput(error.code().as_str().to_owned())
    }

    pub(crate) fn unavailable(text: impl Into<String>) -> Self {
        Self::Unavailable(Failure::transport(text))
    }

    pub(crate) fn lost(text: impl Into<String>) -> Self {
        Self::Lost(Failure::transport(text))
    }
}
