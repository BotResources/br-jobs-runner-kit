use contract_threads::{AiDelivery, Chunk, MessageTurn};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::backend::{Command, CommandBody, Outcome};
use crate::error::Error;
use crate::message::MessageStatus;

pub(crate) type CommandAnswer = Box<dyn Fn(&Command) -> Result<Outcome, Error> + Send + Sync>;
pub(crate) type ChunkAnswer = Box<dyn Fn(Uuid, u64, &Chunk) -> Result<(), Error> + Send + Sync>;
pub(crate) type GraphqlAnswer = Box<dyn Fn(&str, &Value) -> Result<Value, Error> + Send + Sync>;

#[derive(Default)]
pub(crate) struct Script {
    pub(crate) commands: Option<CommandAnswer>,
    pub(crate) chunks: Option<ChunkAnswer>,
    pub(crate) graphql: Option<GraphqlAnswer>,
}

pub(crate) fn default_outcome(command: &Command) -> Outcome {
    let status = match &command.body {
        CommandBody::CreateThread(_) => None,
        CommandBody::Store(store) => Some(match &store.message {
            MessageTurn::Ai(turn) => match turn.delivery {
                AiDelivery::Streaming => MessageStatus::Streaming,
                _ => MessageStatus::Completed,
            },
            _ => MessageStatus::Completed,
        }),
        CommandBody::Complete(_) => Some(MessageStatus::Completed),
        CommandBody::Fail(_) => Some(MessageStatus::Failed),
        CommandBody::Cancel(_) => Some(MessageStatus::Cancelled),
    };
    Outcome {
        command_id: command.id,
        status,
    }
}

pub(crate) fn default_graphql() -> Result<Value, Error> {
    Ok(json!({}))
}
