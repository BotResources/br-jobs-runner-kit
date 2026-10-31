use std::time::Duration;

use br_runner_connection::Connection;
use serde_json::Value;
use uuid::Uuid;

use crate::backend::*;
use crate::error::Error;
use crate::model::*;

pub(crate) struct Live {
    runner_type: String,
    instance_key: String,
    reply_wait: Duration,
}

fn stub<T>() -> BoxFuture<'static, Result<T, Error>> {
    Box::pin(async { Err(Error::invalid("not implemented yet")) })
}

impl Live {
    pub(crate) async fn connect(
        runner_type: &str,
        instance_key: &str,
        reply_wait: Duration,
        _connection: Connection,
    ) -> Result<Self, Error> {
        Ok(Self {
            runner_type: runner_type.to_owned(),
            instance_key: instance_key.to_owned(),
            reply_wait,
        })
    }
}

impl Backend for Live {
    fn runner_type(&self) -> &str {
        &self.runner_type
    }
    fn instance_key(&self) -> &str {
        &self.instance_key
    }
    fn reply_wait(&self) -> Duration {
        self.reply_wait
    }
    fn graphql<'a>(&'a self, _d: &'a str, _v: Value) -> BoxFuture<'a, Result<Value, Error>> {
        stub()
    }
    fn claim(&self, _c: Uuid, _w: Duration) -> BoxFuture<'_, Result<Vec<u8>, Error>> {
        stub()
    }
    fn open_triggers(&self, _a: Duration) -> BoxFuture<'_, Result<Box<dyn TriggerSource>, Error>> {
        stub()
    }
    fn publish_fact(&self, _f: Fact, _w: Duration) -> BoxFuture<'_, Result<(), Error>> {
        stub()
    }
    fn publish_log(&self, _l: LogRecord) -> BoxFuture<'_, Result<(), Error>> {
        stub()
    }
    fn presence_put(&self, _v: String, _s: PresenceStatus, _c: u32) -> BoxFuture<'_, Result<(), Error>> {
        stub()
    }
    fn presence_delete(&self) -> BoxFuture<'_, Result<(), Error>> {
        stub()
    }
    fn presence_ttl(&self) -> BoxFuture<'_, Result<Duration, Error>> {
        stub()
    }
    fn cancel_feed(&self) -> BoxFuture<'_, Result<Box<dyn RawFeed<CancelEvent>>, Error>> {
        stub()
    }
    fn mode_feed(&self) -> BoxFuture<'_, Result<Box<dyn RawFeed<ModeEvent>>, Error>> {
        stub()
    }
    fn arrival_feed(&self, _r: Uuid) -> BoxFuture<'_, Result<Box<dyn RawFeed<ItemArrived>>, Error>> {
        stub()
    }
    fn register_agents(&self, _r: Uuid, _a: Vec<AgentDeclaration>, _w: Duration) -> BoxFuture<'_, Result<(), Error>> {
        stub()
    }
    fn list_agents(&self, _r: Uuid, _w: Duration) -> BoxFuture<'_, Result<Vec<ListedAgent>, Error>> {
        stub()
    }
    fn mark_read(&self, _r: Uuid, _i: Vec<Uuid>, _w: Duration) -> BoxFuture<'_, Result<(), Error>> {
        stub()
    }
    fn child_command(&self, _c: ChildCommand, _w: Duration) -> BoxFuture<'_, Result<(), Error>> {
        stub()
    }
}
