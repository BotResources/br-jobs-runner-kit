use br_agentic_harness_contract::{AgentId, SaveReceipt, ThreadRef, TurnId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ids::{AttemptId, ClientId, SaveKey};
use crate::keys::part_key;
use crate::time::Timestamp;

use super::digest::Sha256Hex;
use super::error::FormatError;
use super::json::{decode, encode};

pub const SCHEMA: u32 = 1;
pub const TAR_FORMAT: &str = "pax";
pub const COMPRESSION: &str = "zstd";
const OBJECT: &str = "manifest";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartReport {
    pub entries: u64,
    pub bytes_raw: u64,
    pub bytes_stored: u64,
    pub sha256_raw: Sha256Hex,
    pub sha256_stored: Sha256Hex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestPart {
    pub index: u32,
    pub key: String,
    pub bytes_raw: u64,
    pub bytes_stored: u64,
    pub sha256_raw: Sha256Hex,
    pub sha256_stored: Sha256Hex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    key: SaveKey,
    parent: Option<TurnId>,
    attempt: AttemptId,
    made_at: Timestamp,
    scope: Vec<String>,
    entries: u64,
    part: ManifestPart,
}

#[derive(Serialize, Deserialize)]
struct ManifestWire {
    schema: u32,
    client: String,
    thread: String,
    agent: String,
    turn: String,
    parent: Option<String>,
    attempt: String,
    made_at: Timestamp,
    scope: Vec<String>,
    tar_format: String,
    compression: String,
    entries: u64,
    parts: Vec<ManifestPart>,
}

fn field(field: &'static str) -> FormatError {
    FormatError::Field {
        object: OBJECT,
        field,
    }
}

fn thread_of(text: &str) -> Result<ThreadRef, FormatError> {
    let uuid = Uuid::parse_str(text).map_err(|_| field("thread"))?;
    let thread = ThreadRef::from_uuid(uuid).map_err(|_| field("thread"))?;
    (thread.to_string() == text)
        .then_some(thread)
        .ok_or_else(|| field("thread"))
}

fn agent_of(text: &str) -> Result<AgentId, FormatError> {
    AgentId::parse(text).map_err(|_| field("agent"))
}

fn turn_of(name: &'static str, text: &str) -> Result<TurnId, FormatError> {
    TurnId::parse(text).map_err(|_| field(name))
}

impl Manifest {
    pub fn new(
        key: SaveKey,
        parent: Option<TurnId>,
        attempt: AttemptId,
        made_at: Timestamp,
        scope: Vec<String>,
        report: PartReport,
    ) -> Self {
        let part = ManifestPart {
            index: 0,
            key: part_key(&key, attempt),
            bytes_raw: report.bytes_raw,
            bytes_stored: report.bytes_stored,
            sha256_raw: report.sha256_raw,
            sha256_stored: report.sha256_stored,
        };
        Self {
            key,
            parent,
            attempt,
            made_at,
            scope,
            entries: report.entries,
            part,
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, FormatError> {
        let wire: ManifestWire = decode(OBJECT, bytes)?;
        if wire.schema != SCHEMA {
            return Err(field("schema"));
        }
        if wire.tar_format != TAR_FORMAT {
            return Err(field("tar_format"));
        }
        if wire.compression != COMPRESSION {
            return Err(field("compression"));
        }
        let client = ClientId::new(wire.client).map_err(|_| field("client"))?;
        let key = SaveKey::new(
            client,
            thread_of(&wire.thread)?,
            agent_of(&wire.agent)?,
            turn_of("turn", &wire.turn)?,
        );
        let attempt = AttemptId::parse(&wire.attempt)?;
        let part = match <[ManifestPart; 1]>::try_from(wire.parts) {
            Ok([part]) if part.index == 0 && part.key == part_key(&key, attempt) => part,
            _ => return Err(field("parts")),
        };
        let parent = wire
            .parent
            .map(|parent| turn_of("parent", &parent))
            .transpose()?;
        Ok(Self {
            key,
            parent,
            attempt,
            made_at: wire.made_at,
            scope: wire.scope,
            entries: wire.entries,
            part,
        })
    }

    pub fn parse_for(bytes: &[u8], asked: &SaveKey) -> Result<Self, FormatError> {
        let manifest = Self::parse(bytes)?;
        if manifest.key == *asked {
            Ok(manifest)
        } else {
            Err(FormatError::OtherKey { object: OBJECT })
        }
    }

    pub fn to_vec(&self) -> Result<Vec<u8>, FormatError> {
        encode(
            OBJECT,
            &ManifestWire {
                schema: SCHEMA,
                client: self.key.client().to_string(),
                thread: self.key.thread().to_string(),
                agent: self.key.agent().to_string(),
                turn: self.key.turn().to_string(),
                parent: self.parent.map(|parent| parent.to_string()),
                attempt: self.attempt.to_string(),
                made_at: self.made_at,
                scope: self.scope.clone(),
                tar_format: TAR_FORMAT.to_owned(),
                compression: COMPRESSION.to_owned(),
                entries: self.entries,
                parts: vec![self.part.clone()],
            },
        )
    }

    pub fn receipt(&self) -> SaveReceipt {
        SaveReceipt::new(
            self.key.turn(),
            self.made_at.to_system_time(),
            self.part.bytes_raw,
        )
    }

    pub fn key(&self) -> &SaveKey {
        &self.key
    }

    pub fn parent(&self) -> Option<TurnId> {
        self.parent
    }

    pub fn attempt(&self) -> AttemptId {
        self.attempt
    }

    pub fn made_at(&self) -> Timestamp {
        self.made_at
    }

    pub fn scope(&self) -> &[String] {
        &self.scope
    }

    pub fn entries(&self) -> u64 {
        self.entries
    }

    pub fn part(&self) -> &ManifestPart {
        &self.part
    }
}
