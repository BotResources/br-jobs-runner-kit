#![allow(dead_code)]

use std::sync::Arc;

use br_agentic_contract::ThreadRef;
use br_core_kernel::ServiceAccountId;
use br_llm_messages::{
    AITurn, AgentId, Author, DeltaOp, EntryId, ResponseDelta, ResponseMetadata, RuntimeNotice,
    TurnEntry, TurnId,
};
use br_runner_threads::transport::{Answer, Confirmed, MemoryBus, MemoryGraphql, Reply};
use br_runner_threads::{
    Closing, ClosureKind, MessageStatus, OpenStream, StreamHandle, ThreadsClient,
};
use uuid::Uuid;

pub mod closings;
pub mod nats;
pub mod server;

pub fn v7() -> Uuid {
    Uuid::now_v7()
}

pub fn not_v7() -> Uuid {
    Uuid::from_u128(0x1111_1111_1111_4111_8111_1111_1111_1111)
}

pub fn v7_with_a_foreign_variant() -> Uuid {
    Uuid::from_u128(0x0192_b3c4_d5e6_7abc_c123_4567_89ab_cdef)
}

pub fn turn_id() -> TurnId {
    TurnId::from_uuid(v7()).expect("a turn id")
}

pub fn entry_id() -> EntryId {
    EntryId::from_uuid(v7()).expect("an entry id")
}

pub fn agent_id() -> AgentId {
    AgentId::from_uuid(v7()).expect("an agent id")
}

pub fn actor() -> ServiceAccountId {
    ServiceAccountId(Uuid::from_u128(0xA0))
}

pub struct World {
    pub client: ThreadsClient,
    pub bus: MemoryBus,
    pub graphql: MemoryGraphql,
    pub thread: ThreadRef,
}

pub fn world() -> World {
    let bus = MemoryBus::new();
    let graphql = MemoryGraphql::new();
    let client = ThreadsClient::new(actor(), Arc::new(bus.clone()), Arc::new(graphql.clone()));
    World {
        client,
        bus,
        graphql,
        thread: ThreadRef::new(v7()),
    }
}

pub fn confirmed(command_id: Uuid, status: Option<MessageStatus>) -> Reply {
    Reply::Answer(Answer::Confirmed(Confirmed { command_id, status }))
}

pub fn open_request(world: &World) -> OpenStream {
    OpenStream {
        command_id: v7(),
        thread: world.thread,
        message_id: turn_id(),
        parent_message_id: Some(v7()),
        agent_id: agent_id(),
        author_name: "Analyst".to_owned(),
        job_id: None,
        acting_user_id: None,
        wait: None,
    }
}

pub async fn open(world: &World) -> StreamHandle {
    world
        .client
        .open_stream(&open_request(world))
        .await
        .expect("the stream opens")
}

pub fn notice() -> TurnEntry {
    TurnEntry::RuntimeNotice(RuntimeNotice::new(entry_id(), "heads up").expect("a notice"))
}

pub fn delta_for(message: Uuid) -> ResponseDelta {
    ResponseDelta::new(
        TurnId::from_uuid(message).expect("a turn id"),
        entry_id(),
        DeltaOp::response_started(ResponseMetadata::new()),
    )
}

pub fn closing(handle: &StreamHandle, thread: ThreadRef, kind: ClosureKind) -> Closing {
    Closing {
        command_id: v7(),
        thread,
        message_id: handle.message_id(),
        kind,
        detail: (kind == ClosureKind::Failed).then(|| "the model failed".to_owned()),
        acting_user_id: None,
        wait: None,
    }
}

pub fn empty_turn() -> AITurn {
    AITurn::open(
        turn_id(),
        agent_id(),
        Author::new("Analyst").expect("an author"),
    )
}

pub fn reply(
    status: u16,
    body: serde_json::Value,
) -> Result<br_runner_threads::transport::HttpReply, br_runner_threads::Error> {
    Ok(br_runner_threads::transport::HttpReply {
        status,
        body: serde_json::to_vec(&body).expect("json"),
    })
}

pub fn query(world: &World, agent: bool) -> br_runner_threads::ContextQuery {
    br_runner_threads::ContextQuery {
        thread: world.thread,
        agent: agent.then(agent_id),
    }
}

pub fn new_thread(world: &World) -> br_runner_threads::NewThread {
    br_runner_threads::NewThread {
        command_id: v7(),
        thread: world.thread,
        owner_id: None,
        member_ids: Vec::new(),
        title: None,
        resources: std::collections::BTreeMap::new(),
        origin: None,
        acting_user_id: None,
        wait: None,
    }
}
