mod arrivals;
mod cancel;
mod commands;
mod connect;
mod fail;
mod mode;
mod mode_sink;
mod outcomes;
mod presence;
mod publish;
mod surface;
#[cfg(test)]
mod tests;
mod triggers;

use std::time::Duration;

use async_nats::jetstream::Context;
use async_nats::jetstream::kv::Store;
use br_runner_connection::Connection;
use br_util_nats_fabric::Fabric;
use contract_jobs::segment::SubjectSegment;
use serde_json::Value;
use uuid::Uuid;

use crate::arrival::ArrivalSettings;
use crate::backend::*;
use crate::error::{Error, Operation};
use crate::from_connection;
use crate::model::*;
use crate::runner_type::RunnerType;

pub(crate) struct Live {
    runner_type: RunnerType,
    instance_key: String,
    reply_wait: Duration,
    segments: Segments,
    connection: Connection,
    wire: Wire,
    presence: Store,
    cancel: Store,
}

#[derive(Clone)]
struct Segments {
    runner_type: SubjectSegment,
    instance_key: SubjectSegment,
}

#[derive(Clone)]
struct Wire {
    jetstream: Context,
    fabric: Fabric,
}

impl Backend for Live {
    fn runner_type(&self) -> &RunnerType {
        &self.runner_type
    }

    fn instance_key(&self) -> &str {
        &self.instance_key
    }

    fn reply_wait(&self) -> Duration {
        self.reply_wait
    }

    fn graphql<'a>(
        &'a self,
        document: &'a str,
        variables: Value,
    ) -> BoxFuture<'a, Result<Value, Error>> {
        Box::pin(async move {
            self.connection
                .graphql(document, variables)
                .await
                .map_err(|error| from_connection::error(error, Operation::Read))
        })
    }

    fn claim(&self, claim_id: Uuid, claim_wait: Duration) -> BoxFuture<'_, Result<Vec<u8>, Error>> {
        Box::pin(self.claim_request(claim_id, claim_wait))
    }

    fn open_triggers(
        &self,
        ack_wait: Duration,
    ) -> BoxFuture<'_, Result<Box<dyn TriggerSource>, Error>> {
        Box::pin(self.trigger_source(ack_wait))
    }

    fn start(
        &self,
        run_id: Uuid,
        start_id: Uuid,
        wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(self.started_request(run_id, start_id, wait))
    }

    fn publish_fact(
        &self,
        fact: Fact,
        message_id: String,
        wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(self.fact(fact, message_id, wait))
    }

    fn publish_log(&self, line: LogRecord) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(self.log(line))
    }

    fn presence_put(
        &self,
        runner_version: String,
        status: PresenceStatus,
        capacity: u32,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(self.put_presence(runner_version, status, capacity))
    }

    fn presence_delete(&self) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(self.delete_presence())
    }

    fn presence_ttl(&self) -> BoxFuture<'_, Result<Duration, Error>> {
        Box::pin(self.time_to_live())
    }

    fn cancel_feed(&self) -> BoxFuture<'_, Result<Box<dyn RawFeed<CancelEvent>>, Error>> {
        Box::pin(self.open_cancel_feed())
    }

    fn mode_feed(&self) -> BoxFuture<'_, Result<Box<dyn RawFeed<ModeEvent>>, Error>> {
        Box::pin(self.open_mode_feed())
    }

    fn arrival_feed(
        &self,
        run_id: Uuid,
        settings: ArrivalSettings,
    ) -> BoxFuture<'_, Result<Box<dyn RawFeed<ItemArrived>>, Error>> {
        Box::pin(self.open_arrival_feed(run_id, settings))
    }

    fn register_agents(
        &self,
        run_id: Uuid,
        agents: Vec<AgentDeclaration>,
        wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(self.register(run_id, agents, wait))
    }

    fn list_agents(
        &self,
        run_id: Uuid,
        wait: Duration,
    ) -> BoxFuture<'_, Result<Vec<ListedAgent>, Error>> {
        Box::pin(self.list(run_id, wait))
    }

    fn mark_read(
        &self,
        run_id: Uuid,
        item_ids: Vec<Uuid>,
        wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(self.mark(run_id, item_ids, wait))
    }

    fn child_session(&self, command: ChildCommand) -> Box<dyn ChildSession + '_> {
        self.child(command)
    }
}
