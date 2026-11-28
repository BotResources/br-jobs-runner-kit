//! A model client that gives scripted deltas and then pends or ends, for the
//! tests that need no HTTP server.

use std::time::Duration;

use br_llm_clients_common_call::{CallEndpoint, ObservedClient, RawUsageCell};
use br_llm_clients_common_http::Endpoint;
use br_llm_clients_core::{
    BoxFuture, CacheRequirement, CallOutcome, CallReport, Client, ClientError, ClientRequest,
    ClientSettings, MaxOutputTokens, ReasoningLevel, StreamSink,
};
use br_llm_messages::{
    DeltaOp, EntryId, ModelName, ResponseDelta, ResponseMetadataUpdate, TurnId, Usage,
};

use super::uuid_of;

pub struct FakeClient {
    settings: ClientSettings,
    endpoint: CallEndpoint,
    deltas: Vec<ResponseDelta>,
    pend: bool,
}

pub fn usage_delta(usage: Usage) -> ResponseDelta {
    ResponseDelta::new(
        TurnId::from_uuid(uuid_of(0x1000)).expect("turn"),
        EntryId::from_uuid(uuid_of(0x2000)).expect("entry"),
        DeltaOp::response_metadata_updated(
            ResponseMetadataUpdate::new()
                .with_usage(usage)
                .with_model(ModelName::new("model-from-delta").expect("model")),
        ),
    )
}

impl FakeClient {
    /// Sends the deltas, then pends for ever (`pend`) or ends with a failure.
    pub fn new(deltas: Vec<ResponseDelta>, pend: bool) -> Self {
        let endpoint = Endpoint::https("https://fake.example/api/").expect("endpoint");
        Self {
            settings: ClientSettings::new(
                ModelName::new("fake-model").expect("model"),
                ReasoningLevel::Off,
                CacheRequirement::NotRequired,
                MaxOutputTokens::new(16).expect("max"),
            ),
            endpoint: CallEndpoint::new("fake", "fake-dialect", &endpoint),
            deltas,
            pend,
        }
    }
}

impl Client for FakeClient {
    fn call<'a>(
        &'a self,
        request: &'a ClientRequest,
        sink: &'a mut dyn StreamSink,
    ) -> BoxFuture<'a, CallOutcome> {
        let cell = RawUsageCell::new();
        Box::pin(async move { self.call_observed(request, sink, &cell).await })
    }

    fn settings(&self) -> &ClientSettings {
        &self.settings
    }
}

impl ObservedClient for FakeClient {
    fn endpoint(&self) -> &CallEndpoint {
        &self.endpoint
    }

    fn call_observed<'a>(
        &'a self,
        _request: &'a ClientRequest,
        sink: &'a mut dyn StreamSink,
        _raw_usage: &'a RawUsageCell,
    ) -> BoxFuture<'a, CallOutcome> {
        Box::pin(async move {
            for delta in &self.deltas {
                sink.send(delta.clone()).await;
            }
            if self.pend {
                std::future::pending::<()>().await;
            }
            CallOutcome::failure(
                CallReport::new(Duration::from_millis(1)),
                ClientError::StreamTruncated,
            )
        })
    }
}
