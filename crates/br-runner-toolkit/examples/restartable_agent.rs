use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_agentic_contract::{
    AgentLabel, ResultStream, ThreadRef, Tool, ToolCode, ToolContent, ToolFault,
};
use br_core_auth::Passport;
use br_core_kernel::ServiceAccountId;
use br_graph_contract::{CancelToken, CancelView, InstanceId};
use br_llm_clients_core::{Parameters, ToolSpec};
use br_llm_messages::{EntryId, OpaqueJson, ToolName};
use br_runner_connection::{Connection, FixedToken};
use br_runner_jobs::{Content, Gate, JobsClient, MarkGate, NodeEntry, mark_gate};
use br_runner_toolkit::{
    DrainReport, InstanceEnded, InstanceLookup, RunnerBuilder, Settings, StartArgs, lifecycle_hook,
};
use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[path = "shared/greeter.rs"]
#[allow(dead_code)] // each example uses a part of the greeter
mod greeter;

#[derive(Clone, Default)]
pub struct Observed {
    pub closed: Arc<Mutex<Vec<String>>>,
    pub stamps: Arc<Mutex<Vec<Stamp>>>,
    pub entries: Arc<Mutex<Vec<Uuid>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    pub job_id: Uuid,
    pub agent_id: String,
}

pub struct WriteContext {
    observed: Observed,
}

pub fn write_context(observed: &Observed) -> WriteContext {
    WriteContext {
        observed: observed.clone(),
    }
}

impl WriteContext {
    pub fn entry_stored(&self, _thread_ref: ThreadRef, entry_id: EntryId) -> Result<(), String> {
        self.observed
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(entry_id.as_uuid());
        Ok(())
    }
}

pub fn stored_gate(context: WriteContext) -> MarkGate {
    mark_gate(move |thread_ref, entry_id| {
        std::future::ready(match context.entry_stored(thread_ref, entry_id) {
            Ok(()) => Gate::Stored,
            Err(_) => Gate::Dropped,
        })
    })
}

pub fn call_service(observed: &Observed, _passport: &Passport, job_id: Uuid, agent_id: String) {
    observed
        .stamps
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(Stamp { job_id, agent_id });
}

pub async fn instance_ended(observed: &Observed, ended: InstanceEnded) {
    let labels: Vec<String> = ended
        .agents
        .iter()
        .map(|agent| format!("{}/{}", agent.key.node, agent.key.label))
        .collect();
    observed
        .closed
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(labels.join(","));
}

struct StampTool {
    lookup: InstanceLookup,
    instance: InstanceId,
    entry: NodeEntry,
    label: AgentLabel,
    observed: Observed,
}

impl ToolCode for StampTool {
    fn call<'a>(
        &'a self,
        _arguments: &'a OpaqueJson,
        _cancel: CancelView,
        _result: &'a mut dyn ResultStream,
    ) -> BoxFuture<'a, Result<ToolContent, ToolFault>> {
        Box::pin(async move {
            let context = self
                .lookup
                .run_context(self.instance)
                .await
                .map_err(ToolFault::new)?;
            let book = context.agent_book().map_err(ToolFault::new)?;
            let agent_id = self
                .entry
                .agent_id(&book, &self.label)
                .map_err(ToolFault::new)?;
            call_service(
                &self.observed,
                context.passport(),
                context.job_id(),
                agent_id.to_string(),
            );
            Ok(ToolContent::new(Vec::new(), false))
        })
    }
}

struct Silent;

impl ResultStream for Silent {
    fn send<'a>(&'a mut self, _fragment: String) -> BoxFuture<'a, ()> {
        Box::pin(std::future::ready(()))
    }
}

fn stamp_tool(
    lookup: InstanceLookup,
    instance: InstanceId,
    observed: &Observed,
) -> Result<Tool, Box<dyn std::error::Error + Send + Sync>> {
    let label = AgentLabel::new("assistant")?;
    let entry = NodeEntry::with_inbox(
        "greeter",
        lookup.book_lookup(),
        label.clone(),
        stored_gate(write_context(observed)),
    );
    let spec = ToolSpec::new(
        ToolName::new("stamp")?,
        "Stamps the job of the run",
        Parameters::new(Vec::new())?,
    )?;
    Ok(Tool::new(
        spec,
        Arc::new(StampTool {
            lookup,
            instance,
            entry,
            label,
            observed: observed.clone(),
        }),
    ))
}

fn greeter_setup(lookup: InstanceLookup, observed: Observed) -> greeter::Setup {
    Arc::new(move |instance| {
        let lookup = lookup.clone();
        let observed = observed.clone();
        Box::pin(async move {
            let Ok(tool) = stamp_tool(lookup, instance, &observed) else {
                return;
            };
            let Ok(arguments) = OpaqueJson::parse("{}") else {
                return;
            };
            let _ = tool
                .call(&arguments, CancelToken::new().view(), &mut Silent)
                .await;
        })
    })
}

pub async fn serve(
    client: JobsClient,
    settings: Settings,
    shutdown: CancellationToken,
    observed: Observed,
) -> Result<DrainReport, Box<dyn std::error::Error + Send + Sync>> {
    let hook_observed = observed.clone();
    let builder =
        RunnerBuilder::new(client, settings).with_lifecycle_hook(lifecycle_hook(move |ended| {
            let observed = hook_observed.clone();
            async move { instance_ended(&observed, ended).await }
        }));
    let setup = greeter_setup(builder.instance_lookup(), observed);
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
    let report = serve(client, settings, shutdown, Observed::default()).await?;
    println!("{} runs left in flight", report.runs.len());
    Ok(())
}
