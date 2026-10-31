use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::error::Error;
use crate::model::{
    AgentDeclaration, CallMode, CancelEvent, Failed, ItemArrived, ListedAgent, LogLevel, ModeEvent,
    PayloadForm, PresenceStatus,
};

pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait TriggerDelivery: Send + Sync {
    fn ack(&self) -> BoxFuture<'_, Result<(), Error>>;
    fn give_back(&self) -> BoxFuture<'_, Result<(), Error>>;
    fn terminate(&self) -> BoxFuture<'_, Result<(), Error>>;
}

pub(crate) struct IncomingTrigger {
    pub raw: Vec<u8>,
    pub delivery: Box<dyn TriggerDelivery>,
}

pub(crate) trait TriggerSource: Send {
    fn next(&mut self) -> BoxFuture<'_, Result<IncomingTrigger, Error>>;
}

pub(crate) trait RawFeed<T>: Send {
    fn next(&mut self) -> BoxFuture<'_, Result<T, Error>>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fact {
    Started {
        run_id: Uuid,
        start_id: Uuid,
    },
    Plan {
        run_id: Uuid,
        steps: Vec<String>,
        declaration_id: Option<Uuid>,
    },
    Step {
        run_id: Uuid,
        index: u32,
        label: String,
        started_at: DateTime<Utc>,
    },
    Completed {
        run_id: Uuid,
    },
    Failed {
        run_id: Uuid,
        failed: Failed,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogRecord {
    pub id: Option<Uuid>,
    pub run_id: Uuid,
    pub step_index: Option<u32>,
    pub level: LogLevel,
    pub message: String,
    pub logged_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateChild {
    pub command_id: Uuid,
    pub job_id: Uuid,
    pub runner_type: String,
    pub mode: CallMode,
    pub config: Option<Value>,
    pub payload: PayloadForm,
    pub passport: Value,
    pub parent_job_id: Uuid,
    pub initial_job_id: Uuid,
    pub run_id: Uuid,
    pub max_attempts: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // a command is built once and moved to the transport, boxing buys nothing
pub enum ChildCommand {
    Create(CreateChild),
    Finish {
        command_id: Uuid,
        job_id: Uuid,
        run_id: Uuid,
    },
    Cancel {
        command_id: Uuid,
        job_id: Uuid,
        run_id: Uuid,
    },
}

pub(crate) trait Backend: Send + Sync {
    fn runner_type(&self) -> &str;

    fn instance_key(&self) -> &str;

    fn reply_wait(&self) -> Duration;

    fn graphql<'a>(
        &'a self,
        document: &'a str,
        variables: Value,
    ) -> BoxFuture<'a, Result<Value, Error>>;

    fn claim(&self, claim_id: Uuid, claim_wait: Duration) -> BoxFuture<'_, Result<Vec<u8>, Error>>;

    fn open_triggers(
        &self,
        ack_wait: Duration,
    ) -> BoxFuture<'_, Result<Box<dyn TriggerSource>, Error>>;

    fn publish_fact(&self, fact: Fact, wait: Duration) -> BoxFuture<'_, Result<(), Error>>;

    fn publish_log(&self, line: LogRecord) -> BoxFuture<'_, Result<(), Error>>;

    fn presence_put(
        &self,
        runner_version: String,
        status: PresenceStatus,
        capacity: u32,
    ) -> BoxFuture<'_, Result<(), Error>>;

    fn presence_delete(&self) -> BoxFuture<'_, Result<(), Error>>;

    fn presence_ttl(&self) -> BoxFuture<'_, Result<Duration, Error>>;

    fn cancel_feed(&self) -> BoxFuture<'_, Result<Box<dyn RawFeed<CancelEvent>>, Error>>;

    fn mode_feed(&self) -> BoxFuture<'_, Result<Box<dyn RawFeed<ModeEvent>>, Error>>;

    fn arrival_feed(
        &self,
        run_id: Uuid,
    ) -> BoxFuture<'_, Result<Box<dyn RawFeed<ItemArrived>>, Error>>;

    fn register_agents(
        &self,
        run_id: Uuid,
        agents: Vec<AgentDeclaration>,
        wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>>;

    fn list_agents(
        &self,
        run_id: Uuid,
        wait: Duration,
    ) -> BoxFuture<'_, Result<Vec<ListedAgent>, Error>>;

    fn mark_read(
        &self,
        run_id: Uuid,
        item_ids: Vec<Uuid>,
        wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>>;

    fn child_command(
        &self,
        command: ChildCommand,
        wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>>;
}
