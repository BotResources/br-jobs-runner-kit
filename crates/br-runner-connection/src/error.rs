use serde_json::{Map, Value};

use crate::credential::CredentialFailure;

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
    pub(crate) fn wait(text: impl Into<String>) -> Self {
        Self {
            cause: Cause::WaitFired,
            text: text.into(),
        }
    }

    pub(crate) fn no_listener(text: impl Into<String>) -> Self {
        Self {
            cause: Cause::NoListener,
            text: text.into(),
        }
    }

    pub(crate) fn transport(text: impl Into<String>) -> Self {
        Self {
            cause: Cause::Transport,
            text: text.into(),
        }
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
pub enum CodeOrigin {
    Service,
    Connection,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Refusal {
    pub code: String,
    pub reason: Option<String>,
    pub params: Map<String, Value>,
    pub message: String,
    pub origin: CodeOrigin,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("{0}")]
    Credential(CredentialFailure),
    #[error("unavailable, {0}")]
    Unavailable(Failure),
    #[error("lost, {0}")]
    Lost(Failure),
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("refused: {}", .0.code)]
    Refused(Refusal),
    #[error("unreadable: {0}")]
    Unreadable(String),
}
