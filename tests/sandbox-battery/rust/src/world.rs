use std::sync::Arc;

use br_agentic_contract::memory::{FixedToolsPort, MemoryThreadAccess, Record};
use br_agentic_contract::{AgentLabel, Fields, InstanceId, PortError, PortFuture, PromptPort};
use br_agentic_harness::{Harness, HarnessBuilder, HarnessHistory, RunEnd, RunLifecycle};
use br_agentic_harness_contract::memory::{HarnessRecord, MemoryHarnessHistory};
use br_agentic_harness_contract::{BoxId, ProcessId, Seed, StartKind, TurnId, TurnRef, WebAccess};
use br_graph_contract::Reason;
use br_llm_messages::{Conversation, ConversationId, Uuid};
use br_runner_sandbox::Sandbox;
use serde::{Deserialize, Serialize};

use crate::error::{Res, data};
use crate::ids::{self, Lane};
use crate::spy::Spy;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    pub n: i64,
}

impl Fields for Doc {
    type Update = i64;

    fn empty_update() -> i64 {
        0
    }

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, update: i64) -> Result<(), Reason> {
        self.n += update;
        Ok(())
    }
}

pub struct FixedPrompt(pub String);

impl PromptPort<Doc> for FixedPrompt {
    fn prompt<'a>(
        &'a self,
        _snapshot: &'a Doc,
        _label: &'a AgentLabel,
    ) -> PortFuture<'a, Result<String, PortError>> {
        Box::pin(async move { Ok(self.0.clone()) })
    }
}

pub struct HarnessWorld {
    pub lane: Lane,
    pub instance: InstanceId,
    pub spy: Spy,
    pub harness: Harness<Doc>,
}

impl HarnessWorld {
    pub fn new(sandbox: Sandbox, web: Option<WebAccess>) -> Res<Self> {
        let lane = Lane::new()?;
        let contract_record = Record::new();
        let record = HarnessRecord::new();
        let conversation = ConversationId::from_uuid(Uuid::now_v7())
            .map_err(|error| data("conversation id", error.to_string()))?;
        let access = MemoryThreadAccess::new(
            &contract_record,
            lane.thread,
            Conversation::new(conversation),
        );
        let history = MemoryHarnessHistory::new(&record, lane.thread, access);
        let spy = Spy::over(sandbox);
        let mut builder: HarnessBuilder<Doc> = Harness::builder(
            Arc::new(history) as Arc<dyn HarnessHistory>,
            Arc::new(FixedPrompt("the prompt of the runner".to_owned())),
            Arc::new(FixedToolsPort::new(&contract_record, Vec::new())),
        )
        .sandbox(spy.port());
        if let Some(web) = web {
            builder = builder.web_access(web);
        }
        Ok(Self {
            lane,
            instance: ids::instance()?,
            spy,
            harness: builder.build(),
        })
    }

    pub fn boxed(&self) -> Res<BoxId> {
        self.spy
            .last_box()
            .ok_or_else(|| data("the harness open", "no box was opened"))
    }

    pub fn first(&self) -> Res<(TurnId, TurnRef)> {
        self.lane.first()
    }

    pub fn child(&self, parent: TurnId) -> Res<(TurnId, TurnRef)> {
        self.lane.child(parent)
    }

    pub async fn open(&self, turn: TurnRef, seed: Seed) -> Result<StartKind, PortError> {
        self.harness.open(self.instance, turn, seed).await
    }

    pub async fn close(&self, end: RunEnd) -> Result<(), PortError> {
        self.harness.close(self.instance, end).await
    }

    pub fn track(&self, process: ProcessId) -> Result<(), PortError> {
        self.harness.track_background(self.instance, process)
    }
}
