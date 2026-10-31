use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::{Fields, PortError, Reason, ThreadAccess, ThreadPort, ThreadRef};
use br_llm_messages::{AgentId, Author, EntryId, IncomingEntry, InputTurn, RuntimeNotice, TurnId};
use br_runner_threads::test_double::MemoryThreads;
use br_runner_threads::{
    Error, IgnoreReports, MemoryQueueStore, ThreadAccessAdapter, ThreadAccessSettings,
    ThreadClient, ThreadPortAdapter, WriteContext, WriteContextFn,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSnapshot {
    pub thread: Option<Uuid>,
}

impl Fields for RunSnapshot {
    type Update = ();

    fn empty_update() -> Self::Update {}

    fn validate(&self) -> Result<(), Reason> {
        Ok(())
    }

    fn apply(&mut self, _update: Self::Update) -> Result<(), Reason> {
        Ok(())
    }
}

pub struct NodePorts {
    pub thread_port: Arc<dyn ThreadPort<RunSnapshot>>,
    pub thread_access: Arc<dyn ThreadAccess>,
    pub adapter: Arc<ThreadAccessAdapter>,
}

pub fn write_context_of(agent: AgentId, author: Author) -> WriteContextFn {
    WriteContextFn::new(move |_thread| {
        let context = WriteContext {
            agent,
            author: author.clone(),
            job_id: None,
            acting_user_id: None,
            impersonator_id: None,
        };
        async move { Ok::<_, PortError>(context) }
    })
}

pub async fn node_ports(
    client: &ThreadClient,
    agent: AgentId,
    author: Author,
) -> Result<NodePorts, Error> {
    let thread_port = ThreadPortAdapter::new(|snapshot: &RunSnapshot| {
        let thread = snapshot.thread;
        Box::pin(async move { Ok(thread) })
    });
    let adapter = Arc::new(
        ThreadAccessAdapter::new(
            client,
            ThreadAccessSettings {
                outcome_wait: Duration::from_secs(10),
                retry_wait: Duration::from_secs(1),
                keepalive_interval: Duration::from_secs(30),
                store: Arc::new(MemoryQueueStore::new()),
                observer: Arc::new(IgnoreReports),
                write_context: write_context_of(agent, author),
            },
        )
        .await?,
    );
    let thread_access: Arc<dyn ThreadAccess> = adapter.clone();
    Ok(NodePorts {
        thread_port: Arc::new(thread_port),
        thread_access,
        adapter,
    })
}

pub async fn thread_of(ports: &NodePorts, snapshot: &RunSnapshot) -> Result<ThreadRef, PortError> {
    ports.thread_port.thread(snapshot).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = MemoryThreads::new().client();
    let agent = AgentId::from_uuid(Uuid::now_v7())?;
    let ports = node_ports(&client, agent, Author::new("Analyst")?).await?;
    let snapshot = RunSnapshot {
        thread: Some(Uuid::now_v7()),
    };
    let thread = thread_of(&ports, &snapshot).await?;
    let notice = RuntimeNotice::new(EntryId::from_uuid(Uuid::now_v7())?, "the run starts")?;
    let turn = InputTurn::new(
        TurnId::from_uuid(Uuid::now_v7())?,
        vec![IncomingEntry::from(notice)],
    )?;
    ports.thread_access.add_input_turn(&thread, turn).await?;
    ports.adapter.instance_ended(agent).await;
    println!("thread {thread} holds the input turn");
    Ok(())
}
