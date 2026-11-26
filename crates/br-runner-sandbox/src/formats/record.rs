use serde::{Deserialize, Serialize};

use crate::ids::AttemptId;
use crate::time::Timestamp;

use super::error::FormatError;
use super::json::{decode, encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordError {
    Exec,
    Store,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordState {
    Started,
    Done {
        ended_at: Timestamp,
    },
    Failed {
        ended_at: Timestamp,
        error: RecordError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    attempt: AttemptId,
    started_at: Timestamp,
    state: RecordState,
}

#[derive(Serialize, Deserialize)]
struct RecordWire {
    state: String,
    attempt: String,
    started_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ended_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<RecordError>,
}

const OBJECT: &str = "record";

impl Record {
    pub fn started(attempt: AttemptId, started_at: Timestamp) -> Self {
        Self {
            attempt,
            started_at,
            state: RecordState::Started,
        }
    }

    pub fn done(&self, ended_at: Timestamp) -> Self {
        Self {
            state: RecordState::Done { ended_at },
            ..self.clone()
        }
    }

    pub fn failed(&self, ended_at: Timestamp, error: RecordError) -> Self {
        Self {
            state: RecordState::Failed { ended_at, error },
            ..self.clone()
        }
    }

    pub fn attempt(&self) -> AttemptId {
        self.attempt
    }

    pub fn started_at(&self) -> Timestamp {
        self.started_at
    }

    pub fn state(&self) -> RecordState {
        self.state
    }

    pub fn age_anchor(&self) -> Timestamp {
        match self.state {
            RecordState::Started => self.started_at,
            RecordState::Done { ended_at } | RecordState::Failed { ended_at, .. } => ended_at,
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, FormatError> {
        let wire: RecordWire = decode(OBJECT, bytes)?;
        let field = |field| FormatError::Field {
            object: OBJECT,
            field,
        };
        let state = match (wire.state.as_str(), wire.ended_at, wire.error) {
            ("started", None, None) => RecordState::Started,
            ("done", Some(ended_at), None) => RecordState::Done { ended_at },
            ("failed", Some(ended_at), Some(error)) => RecordState::Failed { ended_at, error },
            ("started" | "done" | "failed", _, _) => return Err(field("ended_at")),
            _ => return Err(field("state")),
        };
        Ok(Self {
            attempt: AttemptId::parse(&wire.attempt).map_err(|_| field("attempt"))?,
            started_at: wire.started_at,
            state,
        })
    }

    pub fn to_vec(&self) -> Result<Vec<u8>, FormatError> {
        let (state, ended_at, error) = match self.state {
            RecordState::Started => ("started", None, None),
            RecordState::Done { ended_at } => ("done", Some(ended_at), None),
            RecordState::Failed { ended_at, error } => ("failed", Some(ended_at), Some(error)),
        };
        encode(
            OBJECT,
            &RecordWire {
                state: state.to_owned(),
                attempt: self.attempt.to_string(),
                started_at: self.started_at,
                ended_at,
                error,
            },
        )
    }
}
