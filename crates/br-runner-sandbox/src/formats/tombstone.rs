use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

use super::error::FormatError;
use super::json::{decode, encode};

pub const TOMBSTONE_REASON: &str = "expired";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    turn: String,
    made_at: Timestamp,
    expired_at: Timestamp,
    reason: String,
}

impl Tombstone {
    pub fn new(turn: impl ToString, made_at: Timestamp, expired_at: Timestamp) -> Self {
        Self {
            turn: turn.to_string(),
            made_at,
            expired_at,
            reason: TOMBSTONE_REASON.to_owned(),
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, FormatError> {
        decode("tombstone", bytes)
    }

    pub fn to_vec(&self) -> Result<Vec<u8>, FormatError> {
        encode("tombstone", self)
    }

    pub fn expired_at(&self) -> Timestamp {
        self.expired_at
    }

    pub fn made_at(&self) -> Timestamp {
        self.made_at
    }

    pub fn turn(&self) -> &str {
        &self.turn
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}
