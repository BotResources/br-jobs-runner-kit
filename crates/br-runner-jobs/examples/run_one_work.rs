use std::sync::Arc;
use std::time::Duration;

use br_agentic_contract::AgentLabel;
use br_core_kernel::ServiceAccountId;
use br_runner_connection::{Connection, FixedToken};
use br_runner_jobs::{
    AgentBook, Content, DeliveryQueue, Error, IgnoreReports, JobsClient, NodeEntry, QueueSettings,
    Removal, RunnerType, Work, book_lookup,
};
use serde_json::json;
use uuid::Uuid;

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

pub async fn build_client(
    runner_type: RunnerType,
    instance_key: &str,
    connection: Connection,
) -> Result<JobsClient, Error> {
    JobsClient::new(
        runner_type,
        instance_key,
        Duration::from_secs(10),
        connection,
    )
    .await
}

pub async fn start_queue(work: &Work) -> Result<DeliveryQueue, Box<dyn std::error::Error>> {
    let settings = QueueSettings {
        start_id: Uuid::now_v7(),
        outcome_wait: Duration::from_secs(10),
        retry_wait: Duration::from_secs(1),
        observer: Arc::new(IgnoreReports),
    };
    let (queue, started) = work.delivery_queue(settings).await?;
    started.await.map_err(removal_text)?;
    Ok(queue)
}

pub fn node_entry_of(book: &Arc<AgentBook>) -> NodeEntry {
    let book = Arc::clone(book);
    NodeEntry::new(
        "analyst",
        book_lookup(move |_instance| {
            let book = Arc::clone(&book);
            async move { Ok(book) }
        }),
    )
}

pub async fn run_one_work(work: Work) -> Result<String, Box<dyn std::error::Error>> {
    let queue = start_queue(&work).await?;
    work.ack().await?;
    let book = Arc::new(work.agent_book(&queue, Vec::new(), false)?);
    let label = AgentLabel::new("writer")?;
    let agent = node_entry_of(&book).agent_id(&book, &label)?;
    let payload = work.payload().await?;
    let answer = Content {
        message: format!("received: {}", payload.message),
        attachments: Vec::new(),
    };
    let answered = queue.answer(answer);
    let saved = queue.saved_state(json!({ "steps_done": 1, "agents": [agent.as_uuid()] }));
    answered.await.map_err(removal_text)?;
    saved.await.map_err(removal_text)?;
    queue.completed().await.map_err(removal_text)?;
    let left = queue.drained().await;
    Ok(format!("{} writes left", left.waiting.len()))
}

fn removal_text(removal: Removal) -> String {
    format!("a write was removed: {removal:?}")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [
        _,
        endpoint,
        nats_url,
        token,
        account,
        runner_type,
        instance_key,
    ] = args.as_slice()
    else {
        return Err(
            "usage: run_one_work <endpoint> <nats-url> <token> <account-uuid> <runner-type> <instance-key>"
                .into(),
        );
    };
    let bus = async_nats::connect(nats_url).await?;
    let connection = build_connection(endpoint, token, bus, ServiceAccountId(account.parse()?))?;
    let client = build_client(RunnerType::new(runner_type)?, instance_key, connection).await?;
    let mut triggers = client.triggers(Duration::from_secs(30)).await?;
    let work = triggers
        .next()
        .await
        .map_err(|unreadable| unreadable.error)?;
    println!("{}", run_one_work(work).await?);
    Ok(())
}
