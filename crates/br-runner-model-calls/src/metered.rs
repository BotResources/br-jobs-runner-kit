use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use br_llm_clients_core::{
    BoxFuture, CallOutcome, Client, ClientRequest, ClientSettings, StreamSink,
};
use br_llm_messages::{DeltaOp, ModelName, ResponseDelta, Usage};
use uuid::Uuid;

use crate::context::ModelCallContext;
use crate::record::{Ending, ModelCallRecord, Start};
use crate::recorder::ModelCallRecorder;
use crate::{CallEndpoint, ObservedClient, RawUsageCell};

/// The envelope: a [`Client`] that calls the wrapped model client and hands one
/// record of each call to its recorder.
///
/// It deliberately does not implement [`ObservedClient`], so a metered client
/// cannot be wrapped again into a second record of the same call.
pub struct MeteredClient {
    client: Arc<dyn ObservedClient>,
    context: ModelCallContext,
    recorder: ModelCallRecorder,
}

impl MeteredClient {
    pub(crate) fn new(
        client: Arc<dyn ObservedClient>,
        context: ModelCallContext,
        recorder: ModelCallRecorder,
    ) -> Self {
        Self {
            client,
            context,
            recorder,
        }
    }

    /// The endpoint of the wrapped client.
    pub fn endpoint(&self) -> &CallEndpoint {
        self.client.endpoint()
    }
}

impl fmt::Debug for MeteredClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MeteredClient")
            .field("endpoint", self.client.endpoint())
            .field("model", self.client.settings().model())
            .field("context", &self.context)
            .finish_non_exhaustive()
    }
}

impl Client for MeteredClient {
    fn call<'a>(
        &'a self,
        request: &'a ClientRequest,
        sink: &'a mut dyn StreamSink,
    ) -> BoxFuture<'a, CallOutcome> {
        Box::pin(async move {
            let cell = RawUsageCell::new();
            let mut pending = Pending::start(self, cell.clone());
            let mut observing = ObservingSink {
                caller: sink,
                seen: pending.seen.clone(),
            };
            let outcome = self
                .client
                .call_observed(request, &mut observing, &cell)
                .await;
            pending.finish(&outcome);
            outcome
        })
    }

    fn settings(&self) -> &ClientSettings {
        self.client.settings()
    }
}

/// What the deltas of one call told so far.
#[derive(Default)]
struct Seen {
    first_delta: Option<Instant>,
    usage: Option<Usage>,
    model: Option<ModelName>,
}

fn lock(seen: &Mutex<Seen>) -> std::sync::MutexGuard<'_, Seen> {
    seen.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The sink of the envelope: gives every delta to the caller's sink, unchanged
/// and in order, and returns the caller's own future, so it waits exactly as the
/// caller's sink waits.
struct ObservingSink<'a> {
    caller: &'a mut dyn StreamSink,
    seen: Arc<Mutex<Seen>>,
}

impl StreamSink for ObservingSink<'_> {
    fn send<'b>(&'b mut self, delta: ResponseDelta) -> BoxFuture<'b, ()> {
        {
            let mut seen = lock(&self.seen);
            if seen.first_delta.is_none() {
                seen.first_delta = Some(Instant::now());
            }
            let (usage, model) = match delta.op() {
                DeltaOp::ResponseStarted { metadata, .. } => (metadata.usage(), metadata.model()),
                DeltaOp::ResponseMetadataUpdated { update, .. } => (update.usage(), update.model()),
                _ => (None, None),
            };
            if let Some(usage) = usage {
                seen.usage = Some(usage.clone());
            }
            if let Some(model) = model {
                seen.model = Some(model.clone());
            }
        }
        self.caller.send(delta)
    }
}

/// One call between its start and its record. Dropped before
/// [`Pending::finish`], it records an abandoned call when a usage is known.
struct Pending<'a> {
    client: &'a MeteredClient,
    call_id: Uuid,
    started_at: chrono::DateTime<chrono::Utc>,
    started: Instant,
    cell: RawUsageCell,
    seen: Arc<Mutex<Seen>>,
    done: bool,
}

impl<'a> Pending<'a> {
    fn start(client: &'a MeteredClient, cell: RawUsageCell) -> Self {
        Self {
            client,
            call_id: Uuid::now_v7(),
            started_at: chrono::Utc::now(),
            started: Instant::now(),
            cell,
            seen: Arc::default(),
            done: false,
        }
    }

    fn start_part(&self) -> Start {
        Start {
            call_id: self.call_id,
            context: self.client.context.clone(),
            endpoint: self.client.client.endpoint().clone(),
            requested_model: self.client.client.settings().model().clone(),
            started_at: self.started_at,
        }
    }

    fn finish(&mut self, outcome: &CallOutcome) {
        self.done = true;
        let report = outcome.report();
        let ending = Ending {
            answered_model: report.model().cloned(),
            raw_usage: self.cell.get(),
            usage: report.usage().cloned(),
            duration: report.total_duration(),
            time_to_first_token: report.time_to_first_event(),
            success: outcome.result().is_ok(),
            abandoned: false,
        };
        let record = ModelCallRecord::build(self.start_part(), ending);
        self.client.recorder.hand_off(record);
    }

    fn abandon(&mut self) {
        let raw_usage = self.cell.get();
        let (usage, model, first_delta) = {
            let seen = lock(&self.seen);
            (seen.usage.clone(), seen.model.clone(), seen.first_delta)
        };
        if raw_usage.is_none() && usage.is_none() {
            return;
        }
        let ending = Ending {
            answered_model: model,
            raw_usage,
            usage,
            duration: self.started.elapsed(),
            time_to_first_token: first_delta.map(|at| at.saturating_duration_since(self.started)),
            success: false,
            abandoned: true,
        };
        let record = ModelCallRecord::build(self.start_part(), ending);
        self.client.recorder.hand_off(record);
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.abandon();
        }
    }
}
