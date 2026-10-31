use std::sync::Arc;
use std::time::Duration;

use contract_threads::Chunk;
use serde_json::Value;
use uuid::Uuid;

use super::script::{default_graphql, default_outcome};
use super::{Inner, Operation, locked};
use crate::backend::{Backend, BoxFuture, Command, Outcome};
use crate::error::Error;

pub(crate) struct Memory {
    inner: Arc<Inner>,
}

impl Memory {
    pub(crate) fn new(inner: Arc<Inner>) -> Self {
        Self { inner }
    }

    async fn unpaused(&self) {
        let mut paused = self.inner.paused.subscribe();
        while *paused.borrow_and_update() {
            if paused.changed().await.is_err() {
                return;
            }
        }
    }

    fn record(&self, operation: Operation) {
        locked(&self.inner.operations).push(operation);
    }
}

impl Backend for Memory {
    fn command<'a>(
        &'a self,
        command: Command,
        _wait: Duration,
    ) -> BoxFuture<'a, Result<Outcome, Error>> {
        Box::pin(async move {
            self.record(Operation::Command(command.clone()));
            self.unpaused().await;
            let script = locked(&self.inner.script);
            match &script.commands {
                Some(answer) => answer(&command),
                None => Ok(default_outcome(&command)),
            }
        })
    }

    fn chunk<'a>(
        &'a self,
        message_id: Uuid,
        seq: u64,
        chunk: Chunk,
        _wait: Duration,
    ) -> BoxFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            self.record(Operation::Chunk {
                message_id,
                seq,
                chunk: chunk.clone(),
            });
            self.unpaused().await;
            let script = locked(&self.inner.script);
            match &script.chunks {
                Some(answer) => answer(message_id, seq, &chunk),
                None => Ok(()),
            }
        })
    }

    fn graphql<'a>(
        &'a self,
        document: &'a str,
        variables: Value,
    ) -> BoxFuture<'a, Result<Value, Error>> {
        Box::pin(async move {
            self.record(Operation::Graphql {
                document: document.to_owned(),
                variables: variables.clone(),
            });
            self.unpaused().await;
            let script = locked(&self.inner.script);
            match &script.graphql {
                Some(answer) => answer(document, &variables),
                None => default_graphql(),
            }
        })
    }
}
