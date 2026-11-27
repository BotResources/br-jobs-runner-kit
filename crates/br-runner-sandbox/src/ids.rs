use std::fmt;

use br_agentic_harness_contract::{AgentId, ThreadRef, TurnId, TurnRef};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum InvalidClientId {
    #[error("a client id is not blank")]
    Blank,
    #[error("a client id holds no slash")]
    Slash,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientId(String);

impl ClientId {
    pub fn new(text: impl Into<String>) -> Result<Self, InvalidClientId> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(InvalidClientId::Blank);
        }
        if text.contains('/') {
            return Err(InvalidClientId::Slash);
        }
        Ok(Self(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttemptId(Uuid);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("an attempt id is a UUIDv7 in hyphenated lowercase text")]
pub struct InvalidAttemptId;

impl AttemptId {
    pub fn mint() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn from_uuid(uuid: Uuid) -> Result<Self, InvalidAttemptId> {
        if uuid.get_version_num() == 7 {
            Ok(Self(uuid))
        } else {
            Err(InvalidAttemptId)
        }
    }

    pub fn parse(text: &str) -> Result<Self, InvalidAttemptId> {
        let uuid = Uuid::parse_str(text).map_err(|_| InvalidAttemptId)?;
        let attempt = Self::from_uuid(uuid)?;
        if attempt.to_string() == text {
            Ok(attempt)
        } else {
            Err(InvalidAttemptId)
        }
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for AttemptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.hyphenated(), f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SaveKey {
    client: ClientId,
    thread: ThreadRef,
    agent: AgentId,
    turn: TurnId,
}

impl SaveKey {
    pub fn new(client: ClientId, thread: ThreadRef, agent: AgentId, turn: TurnId) -> Self {
        Self {
            client,
            thread,
            agent,
            turn,
        }
    }

    pub fn of_turn(client: &ClientId, turn: &TurnRef) -> Self {
        Self::new(client.clone(), turn.thread, turn.agent, turn.turn)
    }

    pub fn of_parent(client: &ClientId, turn: &TurnRef) -> Option<Self> {
        turn.parent
            .map(|parent| Self::new(client.clone(), turn.thread, turn.agent, parent))
    }

    pub fn client(&self) -> &ClientId {
        &self.client
    }

    pub fn thread(&self) -> ThreadRef {
        self.thread
    }

    pub fn agent(&self) -> AgentId {
        self.agent
    }

    pub fn turn(&self) -> TurnId {
        self.turn
    }
}
