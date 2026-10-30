use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub code: String,
    pub params: Map<String, Value>,
    pub command_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("refused: {}", .0.code)]
    Refused(Refusal),
    #[error("unavailable: {0}")]
    Unavailable(String),
    #[error("answer lost: {0}")]
    AnswerLost(String),
    #[error("stream broken")]
    StreamBroken,
    #[error("stream closed")]
    StreamClosed,
    #[error("no token: {0}")]
    Auth(String),
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("unreadable: {0}")]
    Unreadable(String),
    #[error("infra: {0}")]
    Infra(String),
}

impl Error {
    pub(crate) fn invalid(reason: impl Into<String>) -> Self {
        Self::InvalidInput(reason.into())
    }

    pub(crate) fn library(error: &br_llm_messages::Error) -> Self {
        Self::InvalidInput(error.code().as_str().to_owned())
    }
}
