//! A host's `RecordCommand`: what a contract crate of a consuming service would
//! give, written here so the tests read every field of the record on the bus.

use br_core_kernel::{Actor, ServiceAccountId};
use br_runner_model_calls::{ModelCallRecord, RecordCommand};
use br_util_nats_fabric::{Aggregate, Bc, CommandCoords, EventMetadata, IntegrationCommand, Verb};
use serde::Serialize;
use serde_json::{Map, Number, Value};
use uuid::Uuid;

pub fn coords() -> CommandCoords {
    CommandCoords {
        receiver: Bc::new("meter").expect("bc"),
        aggregate: Aggregate::new("model_call").expect("aggregate"),
        verb: Verb::new("record").expect("verb"),
        version: 1,
    }
}

#[derive(Debug, Serialize)]
pub struct UsageOut {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct RecordPayload {
    pub call_id: Uuid,
    pub job_id: Uuid,
    pub run_id: Uuid,
    pub attempt: u32,
    pub initial_job_id: Uuid,
    pub user_id: Option<Uuid>,
    pub labels: std::collections::BTreeMap<String, String>,
    pub provider: String,
    pub dialect: String,
    pub region: Option<String>,
    pub base_url: String,
    pub requested_model: String,
    pub answered_model: Option<String>,
    pub raw_usage: Option<Map<String, Value>>,
    pub usage: Option<UsageOut>,
    pub service_tier: Option<String>,
    pub inference_geo: Option<String>,
    pub speed: Option<String>,
    pub provider_cost: Option<Number>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub duration_ms: u128,
    pub time_to_first_token_ms: Option<u128>,
    pub success: bool,
    pub abandoned: bool,
}

pub fn payload_of(record: &ModelCallRecord) -> RecordPayload {
    RecordPayload {
        call_id: record.call_id(),
        job_id: record.job_id(),
        run_id: record.run_id(),
        attempt: record.attempt(),
        initial_job_id: record.initial_job_id(),
        user_id: record.user_id(),
        labels: record.labels().clone(),
        provider: record.endpoint().provider().to_owned(),
        dialect: record.endpoint().dialect().to_owned(),
        region: record.endpoint().region().map(str::to_owned),
        base_url: record.endpoint().base_url().to_owned(),
        requested_model: record.requested_model().as_str().to_owned(),
        answered_model: record.answered_model().map(|m| m.as_str().to_owned()),
        raw_usage: record.raw_usage().cloned(),
        usage: record.usage().map(|u| UsageOut {
            input: u.input_tokens(),
            output: u.output_tokens(),
            cache_read: u.cache_read_input_tokens(),
            cache_write: u.cache_creation_input_tokens(),
        }),
        service_tier: record.service_tier().map(str::to_owned),
        inference_geo: record.inference_geo().map(str::to_owned),
        speed: record.speed().map(str::to_owned),
        provider_cost: record.provider_cost().cloned(),
        started_at: record.started_at(),
        duration_ms: record.duration().as_millis(),
        time_to_first_token_ms: record.time_to_first_token().map(|d| d.as_millis()),
        success: record.success(),
        abandoned: record.abandoned(),
    }
}

pub fn actor() -> Actor {
    Actor::Service(ServiceAccountId(Uuid::from_u128(0xBEEF)))
}

pub struct TestCommand {
    coords: CommandCoords,
}

impl TestCommand {
    pub fn new() -> Self {
        Self { coords: coords() }
    }
}

impl RecordCommand for TestCommand {
    type Payload = RecordPayload;

    fn coords(&self) -> &CommandCoords {
        &self.coords
    }

    fn command(&self, record: &ModelCallRecord) -> IntegrationCommand<RecordPayload> {
        IntegrationCommand::new(
            record.call_id(),
            "meter.model_call.record",
            1,
            chrono::Utc::now(),
            EventMetadata::new(actor(), record.job_id()),
            payload_of(record),
        )
    }
}

/// A payload that never serializes.
pub struct Unserializable;

impl Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("this payload never serializes"))
    }
}

pub struct UnserializableCommand {
    coords: CommandCoords,
}

impl UnserializableCommand {
    pub fn new() -> Self {
        Self { coords: coords() }
    }
}

impl RecordCommand for UnserializableCommand {
    type Payload = Unserializable;

    fn coords(&self) -> &CommandCoords {
        &self.coords
    }

    fn command(&self, record: &ModelCallRecord) -> IntegrationCommand<Unserializable> {
        IntegrationCommand::new(
            record.call_id(),
            "meter.model_call.record",
            1,
            chrono::Utc::now(),
            EventMetadata::new(actor(), record.job_id()),
            Unserializable,
        )
    }
}

/// Keeps every record it is asked to map, so a test reads them without a bus.
#[derive(Clone, Default)]
pub struct Capturing {
    pub records: std::sync::Arc<std::sync::Mutex<Vec<ModelCallRecord>>>,
    pub panics: bool,
}

impl Capturing {
    pub fn records(&self) -> Vec<ModelCallRecord> {
        self.records.lock().expect("records").clone()
    }
}

pub struct CapturingCommand {
    coords: CommandCoords,
    capture: Capturing,
}

impl CapturingCommand {
    pub fn new(capture: Capturing) -> Self {
        Self {
            coords: coords(),
            capture,
        }
    }
}

impl RecordCommand for CapturingCommand {
    type Payload = RecordPayload;

    fn coords(&self) -> &CommandCoords {
        &self.coords
    }

    fn command(&self, record: &ModelCallRecord) -> IntegrationCommand<RecordPayload> {
        self.capture
            .records
            .lock()
            .expect("records")
            .push(record.clone());
        assert!(!self.capture.panics, "a host command that panics");
        TestCommand::new().command(record)
    }
}
