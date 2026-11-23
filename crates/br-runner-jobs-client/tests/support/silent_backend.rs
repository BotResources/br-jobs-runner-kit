use std::time::Duration;

use br_runner_jobs_client::{
    AgentDeclaration, ArrivalSettings, Backend, BoxFuture, CancelEvent, ChildCommand, ChildSession,
    Error, Fact, IncomingTrigger, ItemArrived, ListedAgent, LogRecord, ModeEvent, PresenceStatus,
    RawFeed, RunnerType, TriggerDelivery, TriggerSource,
};
use serde_json::Value;
use uuid::Uuid;

struct Delivery;

impl TriggerDelivery for Delivery {
    fn ack(&self) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }

    fn give_back(&self) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }

    fn terminate(&self) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
}

struct Source;

impl TriggerSource for Source {
    fn next(&mut self) -> BoxFuture<'_, Result<IncomingTrigger, Error>> {
        Box::pin(async {
            Ok(IncomingTrigger {
                raw: Vec::new(),
                delivery: Box::new(Delivery),
            })
        })
    }
}

struct Events;

impl<T: Send + 'static> RawFeed<T> for Events {
    fn next(&mut self) -> BoxFuture<'_, Result<T, Error>> {
        Box::pin(async { Err(Error::InvalidInput("no event".to_owned())) })
    }
}

struct Session;

impl ChildSession for Session {
    fn attempt(&mut self, _wait: Duration) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }
}

pub struct Silent {
    pub runner_type: RunnerType,
}

fn refused<T: Send + 'static>() -> BoxFuture<'static, Result<T, Error>> {
    Box::pin(async { Err(Error::InvalidInput("silent".to_owned())) })
}

impl Backend for Silent {
    fn runner_type(&self) -> &RunnerType {
        &self.runner_type
    }

    fn instance_key(&self) -> &str {
        "silent"
    }

    fn reply_wait(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn graphql<'a>(
        &'a self,
        _document: &'a str,
        _variables: Value,
    ) -> BoxFuture<'a, Result<Value, Error>> {
        refused()
    }

    fn claim(
        &self,
        _claim_id: Uuid,
        _claim_wait: Duration,
    ) -> BoxFuture<'_, Result<Vec<u8>, Error>> {
        refused()
    }

    fn open_triggers(
        &self,
        _ack_wait: Duration,
    ) -> BoxFuture<'_, Result<Box<dyn TriggerSource>, Error>> {
        Box::pin(async { Ok(Box::new(Source) as Box<dyn TriggerSource>) })
    }

    fn start(
        &self,
        _run_id: Uuid,
        _start_id: Uuid,
        _wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>> {
        refused()
    }

    fn publish_fact(
        &self,
        _fact: Fact,
        _message_id: String,
        _wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>> {
        refused()
    }

    fn publish_log(&self, _line: LogRecord) -> BoxFuture<'_, Result<(), Error>> {
        refused()
    }

    fn presence_put(
        &self,
        _runner_version: String,
        _status: PresenceStatus,
        _capacity: u32,
    ) -> BoxFuture<'_, Result<(), Error>> {
        refused()
    }

    fn presence_delete(&self) -> BoxFuture<'_, Result<(), Error>> {
        refused()
    }

    fn presence_ttl(&self) -> BoxFuture<'_, Result<Duration, Error>> {
        refused()
    }

    fn cancel_feed(&self) -> BoxFuture<'_, Result<Box<dyn RawFeed<CancelEvent>>, Error>> {
        Box::pin(async { Ok(Box::new(Events) as Box<dyn RawFeed<CancelEvent>>) })
    }

    fn mode_feed(&self) -> BoxFuture<'_, Result<Box<dyn RawFeed<ModeEvent>>, Error>> {
        Box::pin(async { Ok(Box::new(Events) as Box<dyn RawFeed<ModeEvent>>) })
    }

    fn arrival_feed(
        &self,
        _run_id: Uuid,
        _settings: ArrivalSettings,
    ) -> BoxFuture<'_, Result<Box<dyn RawFeed<ItemArrived>>, Error>> {
        Box::pin(async { Ok(Box::new(Events) as Box<dyn RawFeed<ItemArrived>>) })
    }

    fn register_agents(
        &self,
        _run_id: Uuid,
        _agents: Vec<AgentDeclaration>,
        _wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>> {
        refused()
    }

    fn list_agents(
        &self,
        _run_id: Uuid,
        _wait: Duration,
    ) -> BoxFuture<'_, Result<Vec<ListedAgent>, Error>> {
        refused()
    }

    fn mark_read(
        &self,
        _run_id: Uuid,
        _item_ids: Vec<Uuid>,
        _wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>> {
        refused()
    }

    fn child_session(&self, _command: ChildCommand) -> Box<dyn ChildSession + '_> {
        Box::new(Session)
    }
}
