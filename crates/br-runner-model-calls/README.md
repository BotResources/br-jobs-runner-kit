# br-runner-model-calls

An envelope around any model client of a runner. The runner wraps each client of the `br-llm-clients` family in a `MeteredClient` and uses the envelope where it used the client. After each call, the envelope publishes one model call record on the bus, as the integration command of a consuming service (a quota, for example): for a call that succeeds, for a call that fails, and for a call that its caller abandons by a timeout, a cancellation or a drop once a usage of the call is known.

The envelope is itself a `Client` of `br-llm-clients-core`. A runner that calls a client directly and a runner whose calls are made inside a library that takes an `Arc<dyn Client>` use it the same way. Publishing never blocks a model call and never makes one fail.

The library is mechanism: it knows no price, no quota and no consuming service. The contract crate of the consuming service owns the subject and the payload of the command; the host implements `RecordCommand` to map a `ModelCallRecord` to that payload, so this library depends on no contract of a consuming service.

## Install

```toml
br-runner-model-calls = { git = "https://github.com/BotResources/br-jobs-runner-kit", package = "br-runner-model-calls", tag = "br-runner-model-calls/v0.1.0", version = "0.1.0" }
```

It builds on `br-llm-clients-core` 0.5.0 (tag `br-llm-clients-core/v0.5.0`, the tag of the rest of the kit) and `br-llm-clients-common-call` 0.3, which gives the `ObservedClient`, `RawUsageCell` and `CallEndpoint` that this crate re-exports. The bus is reached through the `Fabric` of `br-util-nats-fabric` 0.1.0 that the host gives.

## Quickstart

This is `examples/runner_host.rs`, built by `cargo test` and checked word for word against this page by a test. The module `contract` stands for the contract crate of the consuming service.

```rust,no_run
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
```

## Surface

| Item | Role |
|---|---|
| `ModelCallContext` | `new(job_id, run_id, attempt, initial_job_id)`, `with_user`, `with_label`: what the host knows about the work of one trigger. Carried as given, checked by nobody. |
| `RecordCommand` | The seam the host implements once per consuming contract: `type Payload: Serialize + Send + Sync`, `coords() -> &CommandCoords`, `command(&ModelCallRecord) -> IntegrationCommand<Payload>`. The host sets the command id to the call id. Called once per record, in a task of its own. |
| `PublishSettings` | Attempts, wait of one attempt, first pause (doubling). `default()`: 3 attempts, 2 s, 200 ms. `new` refuses zero attempts or a zero wait with `SettingsError`. |
| `ModelCallRecorder` | `new(Fabric, impl RecordCommand, PublishSettings)`, `wrap(Arc<dyn ObservedClient>, ModelCallContext) -> MeteredClient`, `published()`, `failed_publishes()`, `flush(wait) -> usize`. One per runner; a clone shares it. |
| `MeteredClient` | The envelope: implements `Client` (not `ObservedClient`, so it is never wrapped twice), `endpoint()`. Mints a UUIDv7 call id per call, passes every delta through unchanged, returns the outcome unchanged, never waits for the bus. |
| `ModelCallRecord` | Call id; job, run, attempt, initial job, user, labels; endpoint (provider, dialect, region, base URL); requested and answered model; raw usage and common `Usage`; `service_tier`, `inference_geo`, `speed` (top-level text members of the raw usage) and `provider_cost` (top-level number `cost`); `started_at` (UTC), duration, time to first token; success and abandoned flags. No serialization: the host's payload is what goes on the bus. |
| `SettingsError` | The only error type. |
| `ObservedClient`, `RawUsageCell`, `CallEndpoint` | Re-exported from `br-llm-clients-common-call` 0.3. |

## Rules

- A call that ends with an outcome gives one record, success or failure, with or without a usage.
- A call dropped before its outcome gives one record, flagged abandoned, when a usage is known at the drop (the raw usage cell holds one, or a delta carried a `Usage`), and none otherwise. With no tokio runtime at the drop, the record is counted in `failed_publishes` and a warning is logged.
- Each record is published once on the subject of the coordinates, with the call id as the message id of the bus, so the bus drops a second copy of an attempt sent again. An attempt not confirmed within the wait, or failed, is made again after the pause, up to the number of attempts. The last failure counts the record in `failed_publishes` and logs one warning (call id, job id, subject, kind of failure; no payload, no usage, no credential). A payload that cannot be serialized is not sent again.
- Publishing changes no outcome, no delta and no order of deltas, and the library never panics in a call, a drop or a publish. A panic of the host's `command` ends that publish only: it is counted as failed.

## Tests

`cargo test -p br-runner-model-calls` runs the envelope tests with no bus. The tests against a real JetStream need a `nats-server` binary (`NATS_SERVER_BIN` to point at one): `cargo test -p br-runner-model-calls -- --ignored --test-threads=1`. The model client of the tests runs the call loop of `br-llm-clients-common-call` 0.3 (`run_observed`) against a local HTTP server that replays recorded streams.
