use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_agentic_contract::AgentLabel;
use br_core_kernel::ServiceAccountId;
use br_runner_connection::{Connection, FixedToken};
use br_runner_jobs::{Content, JobsClient, NodeEntry, mark_gate_stored};
use br_runner_toolkit::{
    DrainReport, InstanceEnded, InstanceLookup, RunnerBuilder, Settings, StartArgs, lifecycle_hook,
};
use tokio_util::sync::CancellationToken;

#[path = "shared/greeter.rs"]
#[allow(dead_code)]
mod greeter;

pub type Closed = Arc<Mutex<Vec<String>>>;

pub async fn close_conversation(closed: &Closed, ended: InstanceEnded) {
    let labels: Vec<String> = ended
        .agents
        .iter()
        .map(|agent| format!("{}/{}", agent.key.node, agent.key.label))
        .collect();
    closed
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(labels.join(","));
}

fn greeter_setup(lookup: InstanceLookup) -> greeter::Setup {
    Arc::new(move |instance| {
        let lookup = lookup.clone();
        Box::pin(async move {
            let Ok(book) = lookup.book(instance).await else {
                return;
            };
            let Ok(label) = AgentLabel::new("assistant") else {
                return;
            };
            let entry = NodeEntry::with_inbox(
                "greeter",
                lookup.book_lookup(),
                label.clone(),
                mark_gate_stored(),
            );
            let _ = entry.agent_id(&book, &label);
        })
    })
}

pub async fn serve(
    client: JobsClient,
    settings: Settings,
    shutdown: CancellationToken,
    closed: Closed,
) -> Result<DrainReport, Box<dyn std::error::Error + Send + Sync>> {
    let hook_closed = Arc::clone(&closed);
    let builder =
        RunnerBuilder::new(client, settings).with_lifecycle_hook(lifecycle_hook(move |ended| {
            let closed = Arc::clone(&hook_closed);
            async move { close_conversation(&closed, ended).await }
        }));
    let setup = greeter_setup(builder.instance_lookup());
    let args = StartArgs::new(greeter::graph(Some(setup))?, greeter::starter(), true)
        .with_shutdown(shutdown)
        .with_answer(|state| {
            Ok(Content {
                message: state.fields().lines.join("\n"),
                attachments: Vec::new(),
            })
        });
    Ok(builder.start(args).await?)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
            "usage: restartable_agent <endpoint> <nats-url> <token> <account-uuid> <runner-type> <instance-key>"
                .into(),
        );
    };
    let connection = Connection::new(
        endpoint,
        Arc::new(FixedToken::new(token.as_str())),
        async_nats::connect(nats_url).await?,
        ServiceAccountId(account.parse()?),
        Duration::from_secs(30),
    )?;
    let client = JobsClient::new(
        runner_type,
        instance_key,
        Duration::from_secs(10),
        connection,
    )
    .await?;
    let settings = Settings::builder()
        .runner_version("1.0.0")
        .max_concurrent_runs(4)
        .ack_wait(Duration::from_secs(30))
        .drain_timeout(Duration::from_secs(60))
        .cancel_grace(Duration::from_secs(5))
        .claim_wait(Duration::from_secs(10))
        .outcome_wait(Duration::from_secs(10))
        .retry_wait(Duration::from_secs(1))
        .build()?;
    let shutdown = CancellationToken::new();
    let stopper = shutdown.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        stopper.cancel();
    });
    let report = serve(client, settings, shutdown, Closed::default()).await?;
    println!("{} runs left in flight", report.runs.len());
    Ok(())
}
