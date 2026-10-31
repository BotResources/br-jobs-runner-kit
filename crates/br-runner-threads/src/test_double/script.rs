use contract_threads::{AiDelivery, Chunk, LibraryJson, MessageTurn};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::backend::{Command, CommandBody, Outcome};
use crate::closing::ClosureKind;
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

fn failed_status(closure: Option<&LibraryJson>) -> MessageStatus {
    let kind = closure
        .and_then(|closure| closure.to_closure_reason().ok())
        .and_then(|reason| ClosureKind::of(&reason).ok());
    match kind {
        Some(ClosureKind::Cancelled) => MessageStatus::Cancelled,
        Some(ClosureKind::BudgetExhausted | ClosureKind::Completed) => MessageStatus::Completed,
        _ => MessageStatus::Failed,
    }
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
        CommandBody::Fail(fail) => Some(failed_status(fail.closure.as_ref())),
    };
    Outcome {
        command_id: command.id,
        status,
    }
}

pub(crate) fn default_graphql() -> Result<Value, Error> {
    Ok(json!({}))
}
