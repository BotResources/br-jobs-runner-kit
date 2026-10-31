use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use contract_threads::{Chunk, CompleteMessageV2, CreateThread, FailMessageV2, StoreMessageV2};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::Error;
use crate::message::MessageStatus;

pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommandBody {
    CreateThread(CreateThread),
    Store(StoreMessageV2),
    Complete(CompleteMessageV2),
    Fail(FailMessageV2),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub id: Uuid,
    pub body: CommandBody,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub command_id: Uuid,
    pub status: Option<MessageStatus>,
}

pub(crate) trait Backend: Send + Sync {
    fn command<'a>(
        &'a self,
        command: Command,
        wait: Duration,
    ) -> BoxFuture<'a, Result<Outcome, Error>>;

    fn chunk<'a>(
        &'a self,
        message_id: Uuid,
        seq: u64,
        chunk: Chunk,
        wait: Duration,
    ) -> BoxFuture<'a, Result<(), Error>>;

    fn graphql<'a>(
        &'a self,
        document: &'a str,
        variables: Value,
    ) -> BoxFuture<'a, Result<Value, Error>>;
}
