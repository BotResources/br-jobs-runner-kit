#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_agentic_contract::{
    CancelView, PortFuture, ResultStream, Tool, ToolBlock, ToolContent, ToolFault,
};
use br_graph_contract::{CancelHandle, CancelToken};
use br_llm_clients_core::Parameters;
use br_llm_messages::{OpaqueJson, ToolName};
use br_runner_as_tool::{RunnerTool, ToolMode};
use br_runner_jobs::test_double::{MemoryCall, MemoryScript, ScriptedJobs};
use br_runner_jobs::{CallerHandle, Content, CreateChildInput, Error, Operation, SyncEnd};
use serde_json::Value;
use tokio::task::JoinHandle;
use uuid::Uuid;

pub type Outcome = Result<ToolContent, ToolFault>;

pub fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

pub fn content(message: &str) -> Content {
    Content {
        message: message.to_owned(),
        attachments: Vec::new(),
    }
}

pub fn completed(message: &str) -> SyncEnd {
    SyncEnd::Completed {
        inline: Some(content(message)),
        result_ref: None,
    }
}

pub fn builder(runner_type: &str, mode: ToolMode, caller: CallerHandle) -> RunnerTool {
    RunnerTool::new(
        runner_type,
        ToolName::new("ask_analyst").expect("a tool name"),
        "Asks the analyst runner to do a piece of work.",
        Parameters::new(Vec::new()).expect("parameters"),
        mode,
        caller,
    )
}

pub fn sync_tool(caller: CallerHandle) -> Tool {
    builder("analyst", ToolMode::Sync, caller)
        .build()
        .expect("a tool")
}

pub fn background_tool(caller: CallerHandle, address: Uuid) -> Tool {
    builder(
        "analyst",
        ToolMode::Background {
            caller_address: address,
        },
        caller,
    )
    .build()
    .expect("a tool")
}

pub struct Fragments(pub Arc<Mutex<Vec<String>>>);

impl ResultStream for Fragments {
    fn send<'a>(&'a mut self, fragment: String) -> PortFuture<'a, ()> {
        self.0.lock().expect("fragments").push(fragment);
        Box::pin(async {})
    }
}

pub struct Running {
    pub task: JoinHandle<Outcome>,
    pub cancel: CancelHandle,
    pub fragments: Arc<Mutex<Vec<String>>>,
}

pub fn spawn_call(tool: &Tool, arguments: Value) -> Running {
    let token = CancelToken::new();
    start(tool, arguments, token.view(), token.handle().clone())
}

pub fn spawn_with_cancel(tool: &Tool, arguments: Value, token: &CancelToken) -> Running {
    start(tool, arguments, token.view(), token.handle().clone())
}

fn start(tool: &Tool, arguments: Value, view: CancelView, cancel: CancelHandle) -> Running {
    let tool = tool.clone();
    let fragments = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&fragments);
    let task = tokio::spawn(async move {
        let arguments = OpaqueJson::from_serialize(&arguments).expect("arguments");
        let mut stream = Fragments(sink);
        tool.call(&arguments, view, &mut stream).await
    });
    Running {
        task,
        cancel,
        fragments,
    }
}

impl Running {
    pub async fn finished(self) -> Outcome {
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .expect("the call did not end")
            .expect("the call task")
    }

    pub async fn pending(&self) -> bool {
        settle().await;
        !self.task.is_finished()
    }
}

pub async fn settle() {
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}

pub async fn create_of(script: &MemoryScript) -> CreateChildInput {
    for _ in 0..10_000 {
        if let Some(input) = script.calls().into_iter().find_map(|call| match call {
            MemoryCall::Create(input) => Some(input),
            _ => None,
        }) {
            return input;
        }
        tokio::task::yield_now().await;
    }
    panic!("no create was handed over");
}

pub fn count(script: &MemoryScript, wanted: fn(&MemoryCall) -> bool) -> usize {
    script.calls().iter().filter(|call| wanted(call)).count()
}

pub fn is_finish(call: &MemoryCall) -> bool {
    matches!(call, MemoryCall::Finish { .. })
}

pub fn is_cancel(call: &MemoryCall) -> bool {
    matches!(call, MemoryCall::Cancel { .. })
}

pub fn is_create(call: &MemoryCall) -> bool {
    matches!(call, MemoryCall::Create(_))
}

pub fn is_upload(call: &MemoryCall) -> bool {
    matches!(call, MemoryCall::Upload { .. })
}

pub fn is_read(call: &MemoryCall) -> bool {
    matches!(call, MemoryCall::ReadResult { .. })
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
            MemoryCall::EndWait { .. } => "wait",
            _ => "other",
        })
        .collect()
}

pub fn texts(content: &ToolContent) -> Vec<String> {
    content
        .content()
        .iter()
        .map(|block| match block {
            ToolBlock::Text(text) => text.as_str().to_owned(),
            other => panic!("not a text block: {other:?}"),
        })
        .collect()
}

pub async fn refused(reason: &str) -> Error {
    let jobs = ScriptedJobs::new("analyst", "pod-1");
    let client = jobs.client();
    jobs.push_claim_reply(Ok(format!(
        r#"{{"version":2,"reason":"{reason}","retry_after_ms":250}}"#
    )
    .into_bytes()));
    match client.claim(id(900), Duration::from_secs(1)).await {
        Err(error @ Error::Refused(_)) => error,
        other => panic!("{:?}", other.map(|_| ())),
    }
}

pub async fn refused_saying(reason: &str, message: &str) -> Error {
    match refused(reason).await {
        Error::Refused(mut refusal) => {
            refusal.operation = Operation::Create;
            refusal.message = message.to_owned();
            Error::Refused(refusal)
        }
        other => other,
    }
}
