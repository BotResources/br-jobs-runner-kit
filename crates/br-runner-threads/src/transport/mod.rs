use std::future::Future;
use std::pin::Pin;

use br_core_kernel::ServiceAccountId;
use serde_json::Value;
use uuid::Uuid;

use crate::error::{Error, Refusal};
use crate::message::MessageStatus;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq)]
pub struct BusCommand {
    pub command_id: Uuid,
    pub aggregate: &'static str,
    pub verb: &'static str,
    pub version: u8,
    pub actor: ServiceAccountId,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Confirmed {
    pub command_id: Uuid,
    pub status: Option<MessageStatus>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Confirmed(Confirmed),
    Refused(Refusal),
}

pub type AnswerWait = BoxFuture<'static, Result<Answer, Error>>;

#[derive(Debug, Clone, PartialEq)]
pub struct OutboundChunk {
    pub message_id: Uuid,
    pub seq: u64,
    pub frame: Value,
}

pub trait BusTransport: Send + Sync {
    fn ready(&self) -> BoxFuture<'_, Result<(), Error>>;

    fn send(&self, command: BusCommand) -> BoxFuture<'_, Result<AnswerWait, Error>>;

    fn publish_chunk(&self, chunk: OutboundChunk) -> BoxFuture<'_, Result<(), Error>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    pub status: u16,
    pub body: Vec<u8>,
}

pub trait GraphqlTransport: Send + Sync {
    fn post(&self, body: Vec<u8>) -> BoxFuture<'_, Result<HttpReply, Error>>;
}

mod http;
mod memory;
mod nats;
mod nats_answers;

pub use http::HttpGraphql;
pub use memory::{ChunkMode, MemoryBus, MemoryGraphql, ReadyMode, Reply};
pub use nats::NatsBus;
