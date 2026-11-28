//! A model client of the family for the tests: the call loop of
//! `br-llm-clients-common-call` (`run` / `run_observed`) with a small replay
//! dialect whose frames are JSON objects naming the provider event to give.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use br_llm_clients_common_call::{
    CallEndpoint, CallSpec, Dialect, EncodedRequest, End, ErrorInfo, Headers, ObservedClient,
    Payload, PayloadDecoder, PayloadError, ProviderBlock, ProviderEvent, RawUsageCell, run,
    run_observed,
};
use br_llm_clients_common_http::{Endpoint, HttpClient, HttpConfig, NoAuthorizer};
use br_llm_clients_core::{
    BoxFuture, CacheRequirement, CallOutcome, Client, ClientError, ClientRequest, ClientSettings,
    IdSource, MaxOutputTokens, OutputMode, ReasoningLevel, RenderedHistory, StreamSink,
};
use br_llm_messages::{BlockId, ModelName, ResponseEnding, Usage};
use serde_json::Value;

use super::uuid_of;

pub const PROVIDER: &str = "test-provider";
pub const DIALECT: &str = "test-replay";
pub const REGION: &str = "eu-test-1";
pub const REQUESTED: &str = "eu.vendor.model-small-1";

struct Replay;

impl Dialect for Replay {
    fn encode(
        &self,
        _request: &ClientRequest,
        _history: &RenderedHistory,
    ) -> Result<EncodedRequest, ClientError> {
        Ok(EncodedRequest::new(b"{\"ask\":1}".to_vec())
            .with_header("content-type", "application/json"))
    }

    fn decoder(&self, _output: &OutputMode) -> Box<dyn PayloadDecoder> {
        Box::new(ReplayDecoder)
    }

    fn error_code(&self, body: &str) -> Option<String> {
        let value: Value = serde_json::from_str(body).ok()?;
        value["error"]["type"].as_str().map(str::to_owned)
    }

    fn refine_error(&self, _info: &ErrorInfo) -> Option<ClientError> {
        None
    }

    fn request_id(&self, headers: &Headers) -> Option<String> {
        headers.get_text("request-id").map(str::to_owned)
    }
}

struct ReplayDecoder;

fn count(frame: &Value, key: &str) -> Option<u64> {
    frame.get(key).and_then(Value::as_u64)
}

fn usage_of(frame: &Value) -> Usage {
    let mut usage = Usage::of_input_tokens(count(frame, "input").unwrap_or(0));
    if let Some(n) = count(frame, "output") {
        usage = usage.with_output_tokens(n);
    }
    if let Some(n) = count(frame, "cache_read") {
        usage = usage.with_cache_read_input_tokens(n);
    }
    if let Some(n) = count(frame, "cache_write") {
        usage = usage.with_cache_creation_input_tokens(n);
    }
    usage
}

impl PayloadDecoder for ReplayDecoder {
    fn payload(&mut self, payload: Payload) -> Result<Vec<ProviderEvent>, PayloadError> {
        let frame: Value =
            serde_json::from_str(payload.data()).map_err(|e| PayloadError::new(e.to_string()))?;
        let key = frame["key"].as_str().unwrap_or("0").to_owned();
        let event = match frame["type"].as_str() {
            Some("start") => ProviderEvent::BlockStart {
                key,
                block: ProviderBlock::Text,
            },
            Some("text") => ProviderEvent::TextDelta {
                key,
                text: frame["text"].as_str().unwrap_or_default().to_owned(),
            },
            Some("end") => ProviderEvent::BlockEnd { key },
            Some("stop") => ProviderEvent::Stop(ResponseEnding::finished()),
            Some("usage") => ProviderEvent::Usage(usage_of(&frame)),
            Some("raw_usage") => match frame.get("usage") {
                Some(Value::Object(raw)) => ProviderEvent::RawUsage(raw.clone()),
                _ => return Err(PayloadError::new("raw_usage without an object")),
            },
            Some("model") => {
                ProviderEvent::Model(frame["model"].as_str().unwrap_or_default().to_owned())
            }
            Some("error") => ProviderEvent::Error(
                ErrorInfo::new(frame["text"].as_str().unwrap_or_default())
                    .with_code(frame["code"].as_str().unwrap_or("error")),
            ),
            Some("complete") => ProviderEvent::Complete,
            _ => return Err(PayloadError::new("unknown frame")),
        };
        Ok(vec![event])
    }

    fn end(&mut self) -> Result<End, PayloadError> {
        Ok(End::Events(Vec::new()))
    }
}

/// Block ids from a counter, so two clients built alike give equal deltas.
struct CountingIds(AtomicU64);

impl IdSource for CountingIds {
    fn next_block_id(&self) -> BlockId {
        BlockId::from_uuid(uuid_of(0x5000 + self.0.fetch_add(1, Ordering::SeqCst)))
            .expect("a block id")
    }
}

pub struct TestClient {
    spec: CallSpec,
    settings: ClientSettings,
    endpoint: CallEndpoint,
}

pub fn test_client(base: &str) -> TestClient {
    let long = Duration::from_secs(60);
    let endpoint = Endpoint::plain_http(base).expect("endpoint");
    let http = HttpClient::new(HttpConfig::new(long, long, long)).expect("http client");
    let spec = CallSpec::new(
        Arc::new(Replay),
        endpoint.clone(),
        Arc::new(NoAuthorizer),
        http,
    )
    .with_id_source(Arc::new(CountingIds(AtomicU64::new(1))));
    let settings = ClientSettings::new(
        ModelName::new(REQUESTED).expect("model"),
        ReasoningLevel::Off,
        CacheRequirement::NotRequired,
        MaxOutputTokens::new(1024).expect("max"),
    );
    TestClient {
        spec,
        settings,
        endpoint: CallEndpoint::new(PROVIDER, DIALECT, &endpoint).with_region(REGION),
    }
}

impl Client for TestClient {
    fn call<'a>(
        &'a self,
        request: &'a ClientRequest,
        sink: &'a mut dyn StreamSink,
    ) -> BoxFuture<'a, CallOutcome> {
        Box::pin(run(&self.spec, request, sink))
    }

    fn settings(&self) -> &ClientSettings {
        &self.settings
    }
}

impl ObservedClient for TestClient {
    fn endpoint(&self) -> &CallEndpoint {
        &self.endpoint
    }

    fn call_observed<'a>(
        &'a self,
        request: &'a ClientRequest,
        sink: &'a mut dyn StreamSink,
        raw_usage: &'a RawUsageCell,
    ) -> BoxFuture<'a, CallOutcome> {
        Box::pin(run_observed(&self.spec, request, sink, raw_usage))
    }
}
