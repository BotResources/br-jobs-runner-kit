mod backend;
mod script;

use std::sync::{Arc, Mutex};

use contract_threads::Chunk;
use serde_json::Value;
use uuid::Uuid;

pub use crate::backend::{Command, CommandBody, Outcome};
use crate::client::ThreadClient;
use crate::error::Error;
use script::Script;

#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    Command(Command),
    Chunk {
        message_id: Uuid,
        seq: u64,
        chunk: Chunk,
    },
    Graphql {
        document: String,
        variables: Value,
    },
}

pub(crate) struct Inner {
    pub(crate) operations: Mutex<Vec<Operation>>,
    pub(crate) script: Mutex<Script>,
    pub(crate) paused: tokio::sync::watch::Sender<bool>,
}

#[derive(Clone)]
pub struct MemoryThreads {
    inner: Arc<Inner>,
}

impl Default for MemoryThreads {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryThreads {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                operations: Mutex::new(Vec::new()),
                script: Mutex::new(Script::default()),
                paused: tokio::sync::watch::channel(false).0,
            }),
        }
    }

    pub fn client(&self) -> ThreadClient {
        ThreadClient::over(Arc::new(backend::Memory::new(Arc::clone(&self.inner))))
    }

    pub fn operations(&self) -> Vec<Operation> {
        locked(&self.inner.operations).clone()
    }

    pub fn commands(&self) -> Vec<Command> {
        self.operations()
            .into_iter()
            .filter_map(|op| match op {
                Operation::Command(command) => Some(command),
                _ => None,
            })
            .collect()
    }

    pub fn chunks(&self) -> Vec<(Uuid, u64, Chunk)> {
        self.operations()
            .into_iter()
            .filter_map(|op| match op {
                Operation::Chunk {
                    message_id,
                    seq,
                    chunk,
                } => Some((message_id, seq, chunk)),
                _ => None,
            })
            .collect()
    }

    pub fn answer_commands(
        &self,
        answer: impl Fn(&Command) -> Result<Outcome, Error> + Send + Sync + 'static,
    ) {
        locked(&self.inner.script).commands = Some(Box::new(answer));
    }

    pub fn answer_chunks(
        &self,
        answer: impl Fn(Uuid, u64, &Chunk) -> Result<(), Error> + Send + Sync + 'static,
    ) {
        locked(&self.inner.script).chunks = Some(Box::new(answer));
    }

    pub fn answer_graphql(
        &self,
        answer: impl Fn(&str, &Value) -> Result<Value, Error> + Send + Sync + 'static,
    ) {
        locked(&self.inner.script).graphql = Some(Box::new(answer));
    }

    pub fn pause(&self) {
        self.inner.paused.send_replace(true);
    }

    pub fn resume(&self) {
        self.inner.paused.send_replace(false);
    }
}

pub(crate) fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
