use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{Notify, mpsc};
use uuid::Uuid;

use super::scripted_inputs::{Inputs, PresenceRecord};
use crate::backend::{
    Backend, BoxFuture, ChildCommand, ChildSession, Fact, LogRecord, RawFeed, TriggerSource,
};
use crate::client::JobsClient;
use crate::error::{Error, Operation};
use crate::model::{
    AgentDeclaration, CancelEvent, ItemArrived, ListedAgent, ModeEvent, PresenceStatus,
};

mod call;
mod setters;

pub use call::Call;

type Answer<T> = Box<dyn FnMut() -> Result<T, Error> + Send>;

pub(super) struct Script {
    calls: Vec<Call>,
    graphql: VecDeque<Result<Value, Error>>,
    graphql_default: Option<Value>,
    facts: Option<Answer<()>>,
    register: Option<Answer<()>>,
    mark_read: Option<Answer<()>>,
    command: Option<Answer<()>>,
    agents: Vec<ListedAgent>,
}

pub struct ScriptedJobs {
    script: Mutex<Script>,
    runner_type: String,
    instance_key: String,
    reply_wait: Duration,
    hold: Mutex<bool>,
    released: Notify,
    arrivals: Mutex<Option<mpsc::UnboundedReceiver<Result<ItemArrived, Error>>>>,
    arrival_sender: mpsc::UnboundedSender<Result<ItemArrived, Error>>,
    pub(super) inputs: Inputs,
}

impl ScriptedJobs {
    pub fn new(runner_type: &str, instance_key: &str) -> Arc<Self> {
        let (arrival_sender, receiver) = mpsc::unbounded_channel();
        Arc::new(Self {
            script: Mutex::new(Script {
                calls: Vec::new(),
                graphql: VecDeque::new(),
                graphql_default: None,
                facts: None,
                register: None,
                mark_read: None,
                command: None,
                agents: Vec::new(),
            }),
            runner_type: runner_type.to_owned(),
            instance_key: instance_key.to_owned(),
            reply_wait: Duration::from_secs(5),
            hold: Mutex::new(false),
            released: Notify::new(),
            arrivals: Mutex::new(Some(receiver)),
            arrival_sender,
            inputs: Inputs::new(),
        })
    }

    pub fn client(self: &Arc<Self>) -> JobsClient {
        JobsClient::from_backend(Arc::clone(self) as Arc<dyn Backend>)
    }

    pub(super) fn script(&self) -> MutexGuard<'_, Script> {
        self.script.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn push_arrival(&self, arrival: Result<ItemArrived, Error>) {
        let _ = self.arrival_sender.send(arrival);
    }

    async fn gate(&self) {
        loop {
            let notified = self.released.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !*self.hold.lock().unwrap_or_else(|p| p.into_inner()) {
                return;
            }
            notified.await;
        }
    }

    fn record(&self, call: Call) {
        self.script().calls.push(call);
    }
}

pub(super) struct Channel<T>(pub(super) mpsc::UnboundedReceiver<Result<T, Error>>);

impl<T: Send + 'static> RawFeed<T> for Channel<T> {
    fn next(&mut self) -> BoxFuture<'_, Result<T, Error>> {
        Box::pin(async move {
            self.0
                .recv()
                .await
                .unwrap_or_else(|| Err(Error::lost("the scripted feed was dropped")))
        })
    }
}

fn answer<T>(slot: &mut Option<Answer<T>>, default: T) -> Result<T, Error> {
    match slot {
        Some(answer) => answer(),
        None => Ok(default),
    }
}

impl Backend for ScriptedJobs {
    fn runner_type(&self) -> &str {
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
            self.gate().await;
            if let Some(connection) = self.inputs.routed() {
                self.record(Call::Graphql {
                    document: document.to_owned(),
                    variables: variables.clone(),
                });
                return connection
                    .graphql(document, variables)
                    .await
                    .map_err(|error| crate::from_connection::error(error, Operation::Read));
            }
            let mut script = self.script();
            script.calls.push(Call::Graphql {
                document: document.to_owned(),
                variables,
            });
            match script.graphql.pop_front() {
                Some(answer) => answer,
                None => script
                    .graphql_default
                    .clone()
                    .ok_or_else(|| Error::lost("no scripted GraphQL answer")),
            }
        })
    }

    fn claim(&self, claim_id: Uuid, _wait: Duration) -> BoxFuture<'_, Result<Vec<u8>, Error>> {
        Box::pin(async move {
            self.record(Call::Claim { claim_id });
            self.inputs.next_claim()
        })
    }

    fn open_triggers(
        &self,
        _ack_wait: Duration,
    ) -> BoxFuture<'_, Result<Box<dyn TriggerSource>, Error>> {
        Box::pin(async { Ok(self.inputs.trigger_source()) })
    }

    fn publish_fact(&self, fact: Fact, _wait: Duration) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.gate().await;
            let mut script = self.script();
            script.calls.push(Call::Fact(fact));
            answer(&mut script.facts, ())
        })
    }

    fn publish_log(&self, line: LogRecord) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.record(Call::Log(line));
            self.inputs.log_result()
        })
    }

    fn presence_put(
        &self,
        runner_version: String,
        status: PresenceStatus,
        capacity: u32,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.record(Call::PresencePut);
            self.inputs.put_presence(PresenceRecord {
                runner_version,
                status,
                capacity,
            })
        })
    }

    fn presence_delete(&self) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.record(Call::PresenceDelete);
            self.inputs.delete_presence()
        })
    }

    fn presence_ttl(&self) -> BoxFuture<'_, Result<Duration, Error>> {
        Box::pin(async { self.inputs.ttl() })
    }

    fn cancel_feed(&self) -> BoxFuture<'_, Result<Box<dyn RawFeed<CancelEvent>>, Error>> {
        Box::pin(async { self.inputs.cancel_feed() })
    }

    fn mode_feed(&self) -> BoxFuture<'_, Result<Box<dyn RawFeed<ModeEvent>>, Error>> {
        Box::pin(async { self.inputs.mode_feed() })
    }

    fn arrival_feed(
        &self,
        _run_id: Uuid,
    ) -> BoxFuture<'_, Result<Box<dyn RawFeed<ItemArrived>>, Error>> {
        Box::pin(async move {
            let receiver = self
                .arrivals
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take()
                .ok_or_else(|| Error::invalid("the scripted arrival feed is open already"))?;
            Ok(Box::new(Channel(receiver)) as Box<dyn RawFeed<ItemArrived>>)
        })
    }

    fn register_agents(
        &self,
        run_id: Uuid,
        agents: Vec<AgentDeclaration>,
        _wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.gate().await;
            let mut script = self.script();
            script.calls.push(Call::Register { run_id, agents });
            answer(&mut script.register, ())
        })
    }

    fn list_agents(
        &self,
        run_id: Uuid,
        _wait: Duration,
    ) -> BoxFuture<'_, Result<Vec<ListedAgent>, Error>> {
        Box::pin(async move {
            let mut script = self.script();
            script.calls.push(Call::ListAgents { run_id });
            Ok(script.agents.clone())
        })
    }

    fn mark_read(
        &self,
        run_id: Uuid,
        item_ids: Vec<Uuid>,
        _wait: Duration,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.gate().await;
            let mut script = self.script();
            script.calls.push(Call::MarkRead { run_id, item_ids });
            answer(&mut script.mark_read, ())
        })
    }

    fn child_session(&self, command: ChildCommand) -> Box<dyn ChildSession + '_> {
        Box::new(ScriptedChild {
            jobs: self,
            command,
        })
    }
}

struct ScriptedChild<'a> {
    jobs: &'a ScriptedJobs,
    command: ChildCommand,
}

impl ChildSession for ScriptedChild<'_> {
    fn attempt(&mut self, _wait: Duration) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.jobs.gate().await;
            let mut script = self.jobs.script();
            script.calls.push(Call::Command(self.command.clone()));
            answer(&mut script.command, ())
        })
    }
}
