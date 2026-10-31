use std::sync::Arc;

use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{Error, ThreadAccessAdapter};

use super::ids::{agent_id, human_entry, input_turn};
use super::queue_kit::*;
use super::queue_store::*;
use super::world::*;
use br_agentic_contract::ThreadAccess;

pub fn store_error(text: &str) -> Error {
    Error::Store(text.to_owned())
}

pub struct Setup {
    pub store: Arc<FlakyStore>,
    pub recorder: Arc<Recorder>,
    pub agent: br_llm_messages::AgentId,
}

impl Setup {
    pub fn new() -> Self {
        Self {
            store: FlakyStore::new(),
            recorder: Arc::new(Recorder::default()),
            agent: agent_id(),
        }
    }

    pub async fn adapter_on(&self, threads: &MemoryThreads) -> Result<ThreadAccessAdapter, Error> {
        build_adapter(
            &threads.client(),
            self.store.clone(),
            self.recorder.clone(),
            self.agent,
        )
        .await
    }
}

pub async fn leave_inputs_in_the_store(
    setup: &Setup,
    thread: &br_agentic_contract::ThreadRef,
    count: usize,
) -> Vec<uuid::Uuid> {
    let threads = MemoryThreads::new();
    threads.pause();
    let adapter = setup.adapter_on(&threads).await.expect("adapter");
    let mut messages = Vec::new();
    for _ in 0..count {
        let turn = input_turn(vec![human_entry("left")]);
        messages.push(turn.id().as_uuid());
        adapter
            .add_input_turn(thread, turn)
            .await
            .expect("accepted");
    }
    drop(adapter);
    messages
}
