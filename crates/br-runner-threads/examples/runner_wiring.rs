use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::{PortError, ThreadAccess, ThreadRef};
use br_core_kernel::ServiceAccountId;
use br_llm_messages::{AgentId, Author, EntryId, IncomingEntry, InputTurn, RuntimeNotice, TurnId};
use br_runner_connection::{Connection, FixedToken};
use br_runner_threads::{
    EntryOutcome, Error, IgnoreReports, MemoryQueueStore, NewThread, ThreadAccessAdapter,
    ThreadAccessSettings, ThreadClient, WriteContext, WriteContextFn,
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Read,
    LeaveUnread,
}

pub fn build_connection(
    endpoint: &str,
    token: &str,
    bus: async_nats::Client,
    service_account: ServiceAccountId,
) -> Result<Connection, br_runner_connection::Error> {
    Connection::new(
        endpoint,
        Arc::new(FixedToken::new(token)),
        bus,
        service_account,
        Duration::from_secs(30),
    )
}

pub async fn build_thread_access(
    client: &ThreadClient,
    agent: AgentId,
    author: Author,
) -> Result<ThreadAccessAdapter, Error> {
    let write_context = WriteContextFn::new(move |_thread| {
        let context = WriteContext {
            agent,
            author: author.clone(),
            job_id: None,
            acting_user_id: None,
            impersonator_id: None,
        };
        async move { Ok::<_, PortError>(context) }
    });
    let settings = ThreadAccessSettings {
        outcome_wait: Duration::from_secs(10),
        retry_wait: Duration::from_secs(1),
        store: Arc::new(MemoryQueueStore::new()),
        observer: Arc::new(IgnoreReports),
        write_context,
    };
    ThreadAccessAdapter::new(client, settings).await
}

pub async fn node_entry(
    adapter: &ThreadAccessAdapter,
    thread: &ThreadRef,
    mail: EntryId,
    turn: InputTurn,
) -> Result<Mark, PortError> {
    adapter.add_input_turn(thread, turn).await?;
    Ok(match adapter.entry_stored(thread, mail).await {
        EntryOutcome::Stored => Mark::Read,
        EntryOutcome::Dropped => Mark::LeaveUnread,
    })
}

pub async fn lifecycle_hook(adapter: &ThreadAccessAdapter, ended_agents: &[AgentId]) {
    for agent in ended_agents {
        adapter.instance_ended(*agent).await;
    }
}

pub fn mail_turn(text: &str) -> Result<(EntryId, InputTurn), br_llm_messages::Error> {
    let mail = EntryId::from_uuid(Uuid::now_v7())?;
    let notice = RuntimeNotice::new(mail, text)?;
    let turn = InputTurn::new(
        TurnId::from_uuid(Uuid::now_v7())?,
        vec![IncomingEntry::from(notice)],
    )?;
    Ok((mail, turn))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, endpoint, nats_url, token, account] = args.as_slice() else {
        return Err("usage: runner_wiring <endpoint> <nats-url> <token> <account-uuid>".into());
    };
    let bus = async_nats::connect(nats_url).await?;
    let connection = build_connection(endpoint, token, bus, ServiceAccountId(account.parse()?))?;
    let client = ThreadClient::new(&connection, Duration::from_secs(10)).await?;
    let agent = AgentId::from_uuid(Uuid::now_v7())?;
    let adapter = build_thread_access(&client, agent, Author::new("Analyst")?).await?;
    let thread = ThreadRef::from_uuid(Uuid::now_v7())?;
    let creation = NewThread {
        command_id: Uuid::now_v7(),
        thread_id: thread.as_uuid(),
        owner_id: None,
        member_ids: Vec::new(),
        title: None,
        resources: Default::default(),
        origin: None,
        acting_user_id: None,
        wait: Duration::from_secs(10),
    };
    client.create_thread(&creation).await?;
    let (mail, turn) = mail_turn("Please summarize the open tickets")?;
    let mark = node_entry(&adapter, &thread, mail, turn).await?;
    lifecycle_hook(&adapter, &[agent]).await;
    println!("the mail is marked: {mark:?}");
    Ok(())
}
