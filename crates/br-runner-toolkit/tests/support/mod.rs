#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_graph_contract::{
    CancelView, CreateFuture, Edge, Event, Fields, Graph, GraphBuilder, Instance, InstanceId, Node,
    Reason, State, Target,
};
use br_runner_jobs::test_double::{Call, Fact, ScriptedJobs, trigger_json};
use br_runner_jobs::{CancelEvent, DispatchMode, ModeEvent};
use br_runner_toolkit::{
    DrainReport, Error, RunRefusal, RunnerBuilder, Settings, StartArgs, StartedRun, run_start,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Flow {
    pub steps: Vec<String>,
}

pub struct Add(pub String);

impl Fields for Flow {
    type Update = Add;

    fn empty_update() -> Add {
        Add(String::new())
    }

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, update: Add) -> Result<(), Reason> {
        if !update.0.is_empty() {
            self.steps.push(update.0);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub enum Behavior {
    Result(&'static str),
    Fail(&'static str),
    UntilCancelled,
    UntilReleased(Arc<Notify>, &'static str),
}

pub struct Worker {
    pub name: String,
    pub behavior: Behavior,
}

impl Node<Flow> for Worker {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> &str {
        "programmatic"
    }

    fn subkind(&self) -> &str {
        "worker"
    }

    fn create(&self, cancel: CancelView, id: InstanceId) -> CreateFuture<'_, Flow> {
        let behavior = self.behavior.clone();
        Box::pin(async move {
            let (instance, mut reader, sender) = Instance::<Flow>::channel(id);
            tokio::spawn(async move {
                let _ = reader.next().await;
                let event = match behavior {
                    Behavior::Result(text) => Event::Result(Add(text.to_owned())),
                    Behavior::Fail(text) => Event::refused(text),
                    Behavior::UntilCancelled => {
                        cancel.triggered().await;
                        Event::Cancelled
                    }
                    Behavior::UntilReleased(gate, text) => {
                        gate.notified().await;
                        Event::Result(Add(text.to_owned()))
                    }
                };
                sender.finish(event);
            });
            Ok(instance)
        })
    }
}

struct Next {
    source: String,
    targets: Vec<Target>,
}

impl Edge<Flow> for Next {
    fn name(&self) -> &str {
        &self.source
    }

    fn source(&self) -> &str {
        &self.source
    }

    fn targets(&self) -> &[Target] {
        &self.targets
    }

    fn evaluate(&self, _state: &State<Flow>) -> Result<Vec<Target>, br_graph_contract::Cause> {
        Ok(self.targets.clone())
    }
}

pub fn node(name: &str, behavior: Behavior) -> Arc<dyn Node<Flow>> {
    Arc::new(Worker {
        name: name.to_owned(),
        behavior,
    })
}

pub fn chain(nodes: Vec<(&str, Behavior)>) -> Arc<Graph<Flow>> {
    let mut builder = GraphBuilder::<Flow>::new("1").entry(nodes[0].0);
    for (position, (name, behavior)) in nodes.iter().enumerate() {
        let target = nodes
            .get(position + 1)
            .map_or(Target::END, |next| Target::node(next.0));
        builder = builder
            .node(node(name, behavior.clone()))
            .edge(Arc::new(Next {
                source: (*name).to_owned(),
                targets: vec![target],
            }));
    }
    Arc::new(builder.build().expect("a graph"))
}

pub fn single(behavior: Behavior) -> Arc<Graph<Flow>> {
    chain(vec![("work", behavior)])
}

pub fn args(graph: Arc<Graph<Flow>>, can_restart: bool) -> StartArgs<Flow> {
    StartArgs::new(
        graph,
        run_start(|_input| async {
            Ok::<_, RunRefusal>(StartedRun::new(Flow::default(), vec!["work".to_owned()]))
        }),
        can_restart,
    )
}

pub fn settings(slots: u32) -> Settings {
    Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(slots)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(60))
        .cancel_grace(Duration::from_secs(1))
        .claim_wait(Duration::from_secs(5))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1))
        .build()
        .expect("settings")
}

pub fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

pub fn passport() -> Passport {
    Passport::human(
        id(900),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::from_value(json!({ "email": "ocelot@example.test" })).expect("claims"),
    )
}

pub fn trigger(n: u128) -> Vec<u8> {
    serde_json::to_vec(
        &trigger_json(id(100 + n), id(200 + n), id(300), id(400), &passport()).expect("trigger"),
    )
    .expect("bytes")
}

pub fn push_mode(jobs: &ScriptedJobs) {
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Push)));
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
}

pub fn claim_mode(jobs: &ScriptedJobs) {
    jobs.push_mode(Ok(ModeEvent::Mode(DispatchMode::Claim)));
    jobs.push_cancel(Ok(CancelEvent::ReplayComplete));
}

pub struct Running {
    pub jobs: Arc<ScriptedJobs>,
    pub shutdown: CancellationToken,
    pub done: JoinHandle<Result<DrainReport, Error>>,
}

pub fn run(
    jobs: &Arc<ScriptedJobs>,
    builder: impl FnOnce(RunnerBuilder) -> RunnerBuilder,
    slots: u32,
    args: StartArgs<Flow>,
) -> Running {
    let shutdown = CancellationToken::new();
    let runner = builder(RunnerBuilder::new(jobs.client(), settings(slots)));
    let done = tokio::spawn(runner.start(args.with_shutdown(shutdown.clone())));
    Running {
        jobs: Arc::clone(jobs),
        shutdown,
        done,
    }
}

impl Running {
    pub async fn stop(self) -> DrainReport {
        self.shutdown.cancel();
        self.done.await.expect("joined").expect("started")
    }
}

pub async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(600), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the condition came true");
}

pub fn facts(jobs: &ScriptedJobs) -> Vec<Fact> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Fact(fact) => Some(fact),
            _ => None,
        })
        .collect()
}

pub fn has_terminal(jobs: &ScriptedJobs) -> bool {
    facts(jobs)
        .iter()
        .any(|fact| matches!(fact, Fact::Completed { .. } | Fact::Failed { .. }))
}

pub fn kinds(jobs: &ScriptedJobs) -> Vec<&'static str> {
    facts(jobs)
        .iter()
        .map(|fact| match fact {
            Fact::Started { .. } => "started",
            Fact::Plan { .. } => "plan",
            Fact::Step { .. } => "step",
            Fact::Completed { .. } => "completed",
            Fact::Failed { .. } => "failed",
        })
        .collect()
}
