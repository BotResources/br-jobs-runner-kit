use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use br_util_nats_fabric::{Fabric, FabricError, command_subject};
use tokio::sync::Notify;

use crate::ObservedClient;
use crate::command::{ErasedCommand, RecordCommand};
use crate::context::ModelCallContext;
use crate::metered::MeteredClient;
use crate::record::ModelCallRecord;
use crate::settings::PublishSettings;

/// What publishes the records of a runner. One recorder serves every client and
/// every run of the runner; a clone shares it.
#[derive(Clone)]
pub struct ModelCallRecorder {
    inner: Arc<Inner>,
}

struct Inner {
    fabric: Fabric,
    command: Box<dyn ErasedCommand>,
    subject: String,
    settings: PublishSettings,
    published: AtomicU64,
    failed: AtomicU64,
    in_flight: AtomicUsize,
    idle: Notify,
}

impl ModelCallRecorder {
    /// Builds the recorder. It sends nothing and checks nothing on the bus.
    pub fn new<R: RecordCommand>(fabric: Fabric, command: R, settings: PublishSettings) -> Self {
        let subject = command_subject(RecordCommand::coords(&command));
        Self {
            inner: Arc::new(Inner {
                fabric,
                command: Box::new(command),
                subject,
                settings,
                published: AtomicU64::new(0),
                failed: AtomicU64::new(0),
                in_flight: AtomicUsize::new(0),
                idle: Notify::new(),
            }),
        }
    }

    /// Wraps a model client with the context of one run. Wrapping is cheap: wrap
    /// a shared client again for each run.
    pub fn wrap(
        &self,
        client: Arc<dyn ObservedClient>,
        context: ModelCallContext,
    ) -> MeteredClient {
        MeteredClient::new(client, context, self.clone())
    }

    /// The number of records published since the recorder was built.
    pub fn published(&self) -> u64 {
        self.inner.published.load(Ordering::SeqCst)
    }

    /// The number of records that were not published.
    pub fn failed_publishes(&self) -> u64 {
        self.inner.failed.load(Ordering::SeqCst)
    }

    /// Waits for the publishes in flight, up to `wait`, and gives the number
    /// still in flight when it returns.
    pub async fn flush(&self, wait: Duration) -> usize {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let idle = self.inner.idle.notified();
            tokio::pin!(idle);
            idle.as_mut().enable();
            let in_flight = self.inner.in_flight.load(Ordering::SeqCst);
            if in_flight == 0 {
                return 0;
            }
            if tokio::time::timeout_at(deadline, idle).await.is_err() {
                return self.inner.in_flight.load(Ordering::SeqCst);
            }
        }
    }

    /// Hands a record to a task of its own. Never waits, never panics.
    pub(crate) fn hand_off(&self, record: ModelCallRecord) {
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                let flight = InFlight::start(self.inner.clone(), &record);
                runtime.spawn(publish(flight, record));
            }
            Err(_) => {
                self.inner.failed.fetch_add(1, Ordering::SeqCst);
                tracing::warn!(
                    call_id = %record.call_id(),
                    job_id = %record.job_id(),
                    subject = %self.inner.subject,
                    failure = "no tokio runtime at the end of the call",
                    "model call record not published"
                );
            }
        }
    }
}

impl fmt::Debug for ModelCallRecorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelCallRecorder")
            .field("subject", &self.inner.subject)
            .field("settings", &self.inner.settings)
            .finish_non_exhaustive()
    }
}

/// One publish in flight. Its drop always settles the counters, also when the
/// task ends early (a panic of the host's `command`, a runtime shut down).
struct InFlight {
    inner: Arc<Inner>,
    call_id: uuid::Uuid,
    job_id: uuid::Uuid,
    settled: bool,
}

impl InFlight {
    fn start(inner: Arc<Inner>, record: &ModelCallRecord) -> Self {
        inner.in_flight.fetch_add(1, Ordering::SeqCst);
        Self {
            inner,
            call_id: record.call_id(),
            job_id: record.job_id(),
            settled: false,
        }
    }

    fn published(mut self) {
        self.inner.published.fetch_add(1, Ordering::SeqCst);
        self.settled = true;
    }

    fn failed(mut self, failure: &str) {
        self.inner.failed.fetch_add(1, Ordering::SeqCst);
        self.settled = true;
        tracing::warn!(
            call_id = %self.call_id,
            job_id = %self.job_id,
            subject = %self.inner.subject,
            failure,
            "model call record not published"
        );
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if !self.settled {
            self.inner.failed.fetch_add(1, Ordering::SeqCst);
            tracing::warn!(
                call_id = %self.call_id,
                job_id = %self.job_id,
                subject = %self.inner.subject,
                failure = "the publish task ended before the record was settled",
                "model call record not published"
            );
        }
        self.inner.in_flight.fetch_sub(1, Ordering::SeqCst);
        self.inner.idle.notify_waiters();
    }
}

fn failure_kind(error: &FabricError) -> String {
    match error {
        FabricError::Publish { kind, .. } => kind.to_string(),
        FabricError::Serialization(_) => "the payload cannot be serialized".to_owned(),
        _ => "publish failed".to_owned(),
    }
}

async fn publish(flight: InFlight, record: ModelCallRecord) {
    let inner = flight.inner.clone();
    let envelope = match inner.command.envelope(&record) {
        Ok(envelope) => envelope,
        Err(_) => return flight.failed("the payload cannot be serialized"),
    };
    let coords = inner.command.coords();
    let message_id = record.call_id().to_string();
    let settings = inner.settings;
    let mut pause = settings.pause();
    let mut last = String::new();
    for attempt in 1..=settings.attempts() {
        let sent = tokio::time::timeout(
            settings.wait(),
            inner
                .fabric
                .publish_command_with_id(coords, &envelope, &message_id),
        )
        .await;
        match sent {
            Ok(Ok(())) => return flight.published(),
            Ok(Err(error @ FabricError::Serialization(_))) => {
                return flight.failed(&failure_kind(&error));
            }
            Ok(Err(error)) => last = failure_kind(&error),
            Err(_) => last = "not confirmed within the wait".to_owned(),
        }
        if attempt < settings.attempts() {
            tokio::time::sleep(pause).await;
            pause = pause.saturating_mul(2);
        }
    }
    flight.failed(&last);
}
