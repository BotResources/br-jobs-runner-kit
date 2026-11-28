// each test file includes this module and uses only part of it
#![allow(dead_code, unused_imports)]

pub mod client;
pub mod command;
pub mod fake;
pub mod nats;
pub mod server;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_llm_clients_core::{BoxFuture, ClientRequest, OutputMode, StreamSink};
use br_llm_messages::{
    AgentId, Conversation, ConversationId, EntryId, HumanId, HumanMessage, IncomingEntry,
    InputTurn, Perspective, PlainBlock, ResponseDelta, TextBlock, Turn, TurnId,
};
use br_runner_model_calls::ModelCallContext;
use uuid::Uuid;

pub use client::{TestClient, test_client};
pub use command::{Capturing, CapturingCommand, TestCommand, UnserializableCommand, coords};
pub use nats::Bus;
pub use server::{Reply, Step, TestServer};

pub fn uuid_of(n: u64) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 | u128::from(n))
}

pub fn context() -> ModelCallContext {
    ModelCallContext::new(uuid_of(0xA1), uuid_of(0xA2), 2, uuid_of(0xA3))
        .with_user(uuid_of(0xA4))
        .with_label("runner", "ru-example")
        .with_label("step", "draft")
}

pub fn request() -> ClientRequest {
    let agent = AgentId::from_uuid(uuid_of(0x3000)).expect("agent");
    let conversation = ConversationId::from_uuid(uuid_of(0x3200)).expect("conversation");
    let block = br_llm_messages::BlockId::from_uuid(uuid_of(0x4000)).expect("block");
    let content = vec![PlainBlock::Text(
        TextBlock::new(block, "hello").expect("text"),
    )];
    let human = HumanId::from_uuid(uuid_of(0x3100)).expect("human");
    let entry = EntryId::from_uuid(uuid_of(0x3400)).expect("entry");
    let message = HumanMessage::new(entry, human, content).expect("message");
    let turn = TurnId::from_uuid(uuid_of(0x3300)).expect("turn");
    let turns = vec![Turn::InputTurn(
        InputTurn::new(turn, vec![IncomingEntry::from(message)]).expect("input turn"),
    )];
    let conversation = Conversation::from_parts(conversation, turns).expect("conversation");
    let thread = Perspective::of(agent).project(&conversation);
    let reply_turn = TurnId::from_uuid(uuid_of(0x1000)).expect("turn");
    let reply_entry = EntryId::from_uuid(uuid_of(0x2000)).expect("entry");
    ClientRequest::new(
        None,
        thread,
        reply_turn,
        reply_entry,
        Vec::new(),
        OutputMode::Plain,
    )
    .expect("request")
}

/// A sink that keeps every delta, optionally waiting after each one.
#[derive(Default)]
pub struct Collecting {
    pub deltas: Vec<ResponseDelta>,
    pub wait: Option<Duration>,
}

impl StreamSink for Collecting {
    fn send<'a>(&'a mut self, delta: ResponseDelta) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.deltas.push(delta);
            if let Some(wait) = self.wait {
                tokio::time::sleep(wait).await;
            }
        })
    }
}

pub async fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Captures the warnings that the library logs.
#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<u8>>>);

impl Logs {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("logs")).into_owned()
    }
}

impl std::io::Write for Logs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("logs").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
