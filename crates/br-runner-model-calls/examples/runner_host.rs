use std::sync::Arc;
use std::time::Duration;

use br_core_kernel::{Actor, ServiceAccountId};
use br_llm_clients_core::{CallOutcome, Client, ClientRequest, StreamSink};
use br_runner_model_calls::{
    ModelCallContext, ModelCallRecord, ModelCallRecorder, ObservedClient, PublishSettings,
    RecordCommand,
};
use br_util_nats_fabric::{CommandCoords, EventMetadata, Fabric, IntegrationCommand};
use uuid::Uuid;

/// Stands for the contract crate of the consuming service, which owns the
/// coordinates and the payload of its command. A host depends on that crate.
mod contract {
    use br_util_nats_fabric::{Aggregate, Bc, CommandCoords, Verb};
    use serde::Serialize;
    use uuid::Uuid;

    pub const COMMAND_TYPE: &str = "meter.model_call.record";
    pub const VERSION: u8 = 1;

    pub fn coords() -> CommandCoords {
        CommandCoords {
            receiver: Bc::new("meter").expect("a valid bc"),
            aggregate: Aggregate::new("model_call").expect("a valid aggregate"),
            verb: Verb::new("record").expect("a valid verb"),
            version: VERSION,
        }
    }

    #[derive(Serialize)]
    pub struct RecordModelCall {
        pub call_id: Uuid,
        pub job_id: Uuid,
        pub user_id: Option<Uuid>,
        pub provider: String,
        pub requested_model: String,
        pub input_tokens: Option<u64>,
        pub output_tokens: Option<u64>,
        pub raw_usage: Option<serde_json::Map<String, serde_json::Value>>,
        pub started_at: chrono::DateTime<chrono::Utc>,
        pub success: bool,
        pub abandoned: bool,
    }
}

/// The host maps the record to the payload of the contract, once.
pub struct MeterCommand {
    coords: CommandCoords,
    actor: Actor,
}

impl RecordCommand for MeterCommand {
    type Payload = contract::RecordModelCall;

    fn coords(&self) -> &CommandCoords {
        &self.coords
    }

    fn command(&self, record: &ModelCallRecord) -> IntegrationCommand<Self::Payload> {
        let payload = contract::RecordModelCall {
            call_id: record.call_id(),
            job_id: record.job_id(),
            user_id: record.user_id(),
            provider: record.endpoint().provider().to_owned(),
            requested_model: record.requested_model().as_str().to_owned(),
            input_tokens: record.usage().and_then(|u| u.input_tokens()),
            output_tokens: record.usage().and_then(|u| u.output_tokens()),
            raw_usage: record.raw_usage().cloned(),
            started_at: record.started_at(),
            success: record.success(),
            abandoned: record.abandoned(),
        };
        IntegrationCommand::new(
            record.call_id(),
            contract::COMMAND_TYPE,
            contract::VERSION,
            chrono::Utc::now(),
            EventMetadata::new(self.actor, record.job_id()),
            payload,
        )
    }
}

/// One recorder for the whole runner.
pub fn recorder(fabric: Fabric, service_account: Uuid) -> ModelCallRecorder {
    let command = MeterCommand {
        coords: contract::coords(),
        actor: Actor::Service(ServiceAccountId(service_account)),
    };
    ModelCallRecorder::new(fabric, command, PublishSettings::default())
}

/// For each run: wrap the shared client with the context of the trigger, and
/// call the envelope where the client was called. An `Arc<dyn Client>` of it can
/// be given to anything that takes one.
pub async fn one_run(
    recorder: &ModelCallRecorder,
    client: Arc<dyn ObservedClient>,
    (job_id, run_id, attempt, initial_job_id): (Uuid, Uuid, u32, Uuid),
    user_id: Option<Uuid>,
    request: &ClientRequest,
    sink: &mut dyn StreamSink,
) -> CallOutcome {
    let mut context = ModelCallContext::new(job_id, run_id, attempt, initial_job_id)
        .with_label("runner", "ru-example");
    if let Some(user_id) = user_id {
        context = context.with_user(user_id);
    }
    let metered: Arc<dyn Client> = Arc::new(recorder.wrap(client, context));
    metered.call(request, sink).await
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, nats_url] = args.as_slice() else {
        return Err("usage: runner_host <nats-url>".into());
    };
    let fabric = Fabric::connect(nats_url).await?;
    let recorder = recorder(fabric, Uuid::now_v7());
    println!("{recorder:?}");
    // ... the runner serves its runs with `one_run` ...
    let still_in_flight = recorder.flush(Duration::from_secs(5)).await;
    println!(
        "published {}, failed {}, still in flight {still_in_flight}",
        recorder.published(),
        recorder.failed_publishes()
    );
    Ok(())
}
