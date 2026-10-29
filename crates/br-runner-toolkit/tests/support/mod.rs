#![allow(dead_code, clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use br_agentic_contract::{Agent, AgentLabel, AgentPort, ThreadRef};
use br_core_kernel::ServiceAccountId;
use br_graph_contract::{Fields, InstanceId, Reason};
use br_graph_executor::Observation;
use br_llm_messages::{AgentId, Author, Conversation, ConversationId, EntryId, TurnId};
use br_runner_threads::ThreadsClient;
use br_runner_threads::transport::{HttpReply, MemoryBus, MemoryGraphql};
use br_runner_toolkit::{
    AgentRegistrar, AgentRegistration, PortError, PortErrorCode, PortSetRequest, PortSets,
    RunHandle, ToolkitThreadAccess,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    pub note: String,
}

impl Fields for Doc {
    type Update = ();

    fn empty_update() -> Self::Update {}

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, _update: Self::Update) -> Result<(), Reason> {
        Ok(())
    }
}

pub fn doc() -> Doc {
    Doc {
        note: "n".to_owned(),
    }
}

pub fn v7() -> Uuid {
    Uuid::now_v7()
}

pub fn label(text: &str) -> AgentLabel {
    AgentLabel::new(text).expect("a label")
}

pub fn author(text: &str) -> Author {
    Author::new(text).expect("an author")
}

pub fn turn_id() -> TurnId {
    TurnId::from_uuid(v7()).expect("a turn id")
}

pub fn entry_id() -> EntryId {
    EntryId::from_uuid(v7()).expect("an entry id")
}

pub fn instance() -> InstanceId {
    InstanceId::new(v7()).expect("an instance id")
}

pub type Registered = Arc<Mutex<Vec<AgentRegistration>>>;

pub struct Rig {
    pub sets: PortSets,
    pub bus: MemoryBus,
    pub graphql: MemoryGraphql,
    pub run: Uuid,
    pub handle: RunHandle,
    pub registered: Registered,
    pub thread: ThreadRef,
    pub port: Arc<br_runner_toolkit::ToolkitAgentPort<Doc>>,
    pub access: Arc<ToolkitThreadAccess>,
}

pub fn recording_registrar(registered: Registered) -> AgentRegistrar {
    Arc::new(move |registration| {
        registered.lock().expect("registrations").push(registration);
        Box::pin(async { Ok::<(), PortError>(()) })
    })
}

pub fn refusing_registrar() -> AgentRegistrar {
    Arc::new(|_| {
        Box::pin(async {
            Err(PortError::new(
                PortErrorCode::JobsRefused,
                "the agent id is held",
            ))
        })
    })
}

pub fn rig_with(can_restart: Option<bool>, registrar: Option<AgentRegistrar>) -> Rig {
    let sets = PortSets::new(can_restart);
    let set = sets.port_set::<Doc>(
        "writer",
        PortSetRequest::new()
            .author(label("main"), author("Writer"))
            .author(label("helper"), author("Helper"))
            .inbox(label("main")),
    );
    let bus = MemoryBus::new();
    let graphql = MemoryGraphql::new();
    let registered: Registered = Arc::default();
    let threads = ThreadsClient::new(
        ServiceAccountId(Uuid::from_u128(0xA0)),
        Arc::new(bus.clone()),
        Arc::new(graphql.clone()),
    );
    sets.bind(
        threads,
        registrar.unwrap_or_else(|| recording_registrar(Arc::clone(&registered))),
    )
    .expect("bound");
    let run = v7();
    let handle = sets.begin_run(run, None).expect("a run");
    Rig {
        sets,
        bus,
        graphql,
        run,
        handle,
        registered,
        thread: ThreadRef::new(v7()),
        port: set.agent,
        access: set.thread_access,
    }
}

pub fn rig() -> Rig {
    rig_with(None, None)
}

impl Rig {
    pub fn create(&self, node: &str) -> InstanceId {
        let id = instance();
        self.handle
            .observer()
            .observe(Observation::InstanceCreated {
                instance: id,
                node: node.to_owned(),
                kind: "agentic".to_owned(),
                subkind: String::new(),
            });
        id
    }

    pub async fn agent(&self, instance: InstanceId, name: &str) -> Agent {
        self.port
            .agent(&doc(), instance, &label(name))
            .await
            .expect("an agent")
    }

    pub async fn live_agent(&self) -> AgentId {
        let instance = self.create("writer");
        self.agent(instance, "main").await.id
    }

    pub fn show(&self, conversation: &Conversation, agent: AgentId) {
        let projected: Value = serde_json::from_str(
            &conversation
                .project_for(agent)
                .to_json()
                .expect("projection json"),
        )
        .expect("value");
        let body = serde_json::to_vec(&json!({ "data": { "threadsContext": {
            "threadId": self.thread.as_uuid(),
            "conversation": null,
            "projectedThread": projected,
            "members": [],
            "resources": []
        } } }))
        .expect("json");
        self.graphql.reply_with(Ok(HttpReply { status: 200, body }));
    }

    pub fn show_times(&self, conversation: &Conversation, agent: AgentId, times: usize) {
        for _ in 0..times {
            self.show(conversation, agent);
        }
    }

    pub fn empty_thread(&self) -> Conversation {
        Conversation::new(
            ConversationId::from_uuid(self.thread.as_uuid()).expect("a conversation id"),
        )
    }

    pub fn payloads(&self, verb: &str) -> Vec<Value> {
        self.bus
            .commands()
            .into_iter()
            .filter(|command| command.verb == verb)
            .map(|command| command.payload)
            .collect()
    }
}

pub fn text_response(text: &str) -> (br_llm_messages::ModelResponse, br_llm_messages::BlockId) {
    use br_llm_messages::{
        BlockId, ContentBlock, ModelResponse, ResponseEnding, ResponseMetadata, TextBlock,
    };
    let block = BlockId::from_uuid(v7()).expect("a block id");
    let response = ModelResponse::new(
        entry_id(),
        vec![ContentBlock::from(
            TextBlock::new(block, text).expect("text"),
        )],
        ResponseEnding::finished(),
        ResponseMetadata::new(),
    )
    .expect("a response");
    (response, block)
}

pub fn ai_target(turn: TurnId) -> br_agentic_contract::TurnTarget {
    br_agentic_contract::TurnTarget::Ai {
        id: turn,
        author: author("Writer"),
    }
}

impl Rig {
    pub fn sent_deltas(&self) -> Vec<br_llm_messages::ResponseDelta> {
        self.bus
            .chunks()
            .into_iter()
            .filter_map(|chunk| {
                chunk
                    .frame
                    .pointer("/chunk/delta")
                    .cloned()
                    .map(|delta| serde_json::from_value(delta).expect("a delta"))
            })
            .collect()
    }

    pub fn chunk_kinds(&self) -> Vec<String> {
        self.bus
            .chunks()
            .into_iter()
            .map(|chunk| {
                chunk
                    .frame
                    .pointer("/chunk/kind")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_owned()
            })
            .collect()
    }
}
