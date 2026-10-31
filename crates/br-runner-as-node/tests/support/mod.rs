#![allow(dead_code)] // each test crate uses a part of the shared helpers

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use br_graph_contract::{
    CancelToken, CancelView, Cause, Event, EventEnd, Fields, Input, InputEnd, InstanceId, Node,
    Reason, State,
};
use br_runner_as_node::{CallerSource, JobSpec, Outcome, Payload, RunnerAsNode, SubjectSegment};
use br_runner_jobs::test_double::{MemoryCall, MemoryScript, memory_caller};
use br_runner_jobs::{CallerHandle, Content};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    pub question: String,
    pub answer: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowUpdate {
    pub answer: Option<String>,
    pub note: Option<String>,
}

impl Fields for Flow {
    type Update = FlowUpdate;

    fn empty_update() -> FlowUpdate {
        FlowUpdate::default()
    }

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, update: FlowUpdate) -> Result<(), Reason> {
        if update.answer.is_some() {
            self.answer = update.answer;
        }
        if update.note.is_some() {
            self.note = update.note;
        }
        Ok(())
    }
}

pub fn flow(question: &str) -> Flow {
    Flow {
        question: question.to_owned(),
        answer: None,
        note: None,
    }
}

pub fn state_of(question: &str) -> Arc<State<Flow>> {
    Arc::new(State::create(flow(question)).expect("a valid state"))
}

pub fn content(message: &str) -> Content {
    Content {
        message: message.to_owned(),
        attachments: Vec::new(),
    }
}

pub fn runner_type() -> SubjectSegment {
    SubjectSegment::runner_type("analyst").expect("a runner type")
}

pub fn source_of(handle: &CallerHandle) -> impl CallerSource + use<> {
    let handle = handle.clone();
    move |_id: InstanceId| {
        let handle = handle.clone();
        async move { Ok::<_, Cause>(handle) }
    }
}

pub fn describe(outcome: &Outcome) -> FlowUpdate {
    let mut update = FlowUpdate::default();
    match outcome {
        Outcome::Completed {
            inline: Some(content),
            ..
        } => update.answer = Some(content.message.clone()),
        Outcome::Completed {
            result_ref: Some(result),
            ..
        } => update.note = Some(format!("ref:{}", result.as_uuid())),
        Outcome::Completed { .. } => update.note = Some("completed empty".to_owned()),
        Outcome::Failed { message, .. } => update.note = Some(format!("failed:{message}")),
        Outcome::Cancelled { message } => update.note = Some(format!("cancelled:{message}")),
        Outcome::Refusal(refusal) => {
            update.note = Some(format!("refused:{}", refusal.reason));
        }
        _ => update.note = Some("unknown".to_owned()),
    }
    update
}

pub fn standard(source: impl CallerSource) -> RunnerAsNode<Flow> {
    RunnerAsNode::new(
        "ask",
        runner_type(),
        |state: Arc<State<Flow>>| {
            let question = state.fields().question.clone();
            async move { Ok(JobSpec::new(Payload::Kept(content(&question)))) }
        },
        |_state, outcome| {
            let update = describe(&outcome);
            async move { Ok::<_, Cause>(update) }
        },
        source,
    )
}

pub struct Rig {
    pub script: MemoryScript,
    pub handle: CallerHandle,
    pub token: CancelToken,
}

pub fn rig() -> Rig {
    let (handle, script) = memory_caller();
    Rig {
        script,
        handle,
        token: CancelToken::new(),
    }
}

pub struct Running {
    pub input: InputEnd<Flow>,
    pub events: EventEnd<Flow>,
}

pub async fn start(node: &RunnerAsNode<Flow>, cancel: CancelView) -> Running {
    let id = InstanceId::new(Uuid::now_v7()).expect("a UUIDv7");
    let instance = node.create(cancel, id).await.expect("an instance");
    let (input, events) = instance.into_parts();
    Running { input, events }
}

impl Running {
    pub fn give(&self, question: &str) {
        self.input
            .give(Input::new(state_of(question)))
            .expect("the input is open");
    }

    pub async fn event(&mut self) -> Event<Flow> {
        within(self.events.recv()).await.expect("an event")
    }
}

pub async fn within<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("it ended in time")
}

pub async fn stays_pending<T>(future: impl Future<Output = T>) -> bool {
    tokio::time::timeout(Duration::from_millis(100), future)
        .await
        .is_err()
}

pub async fn the_create(script: &MemoryScript) -> br_runner_jobs::CreateChildInput {
    within(async {
        loop {
            for call in script.calls() {
                if let MemoryCall::Create(input) = call {
                    return input;
                }
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
}

pub async fn settle_calls() -> Duration {
    let pause = Duration::from_millis(100);
    tokio::time::sleep(pause).await;
    pause
}

pub fn kinds(script: &MemoryScript) -> Vec<&'static str> {
    script
        .calls()
        .iter()
        .map(|call| match call {
            MemoryCall::Upload { .. } => "upload",
            MemoryCall::Create(_) => "create",
            MemoryCall::Finish { .. } => "finish",
            MemoryCall::Cancel { .. } => "cancel",
            MemoryCall::ReadResult { .. } => "read",
            MemoryCall::EndWait { .. } => "end_wait",
            _ => "other",
        })
        .collect()
}

pub fn update_of(event: Event<Flow>) -> FlowUpdate {
    match event {
        Event::Result(update) => update,
        other => panic!("expected the event Result, got {other:?}"),
    }
}

pub fn error_of(event: Event<Flow>) -> Cause {
    match event {
        Event::Failed(cause) => cause,
        other => panic!("expected the event Failed, got {other:?}"),
    }
}
