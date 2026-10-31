use std::time::Duration;

use br_graph_contract::Fields;
use br_runner_jobs::{JobsClient, QueueObserver};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::drain::DrainReport;
use crate::error::{Error, ErrorCode};
use crate::hook::LifecycleHook;
use crate::instances::InstanceLookup;
use crate::runner::{RunnerBuilder, StartArgs};
use crate::settings::SettingsBuilder;
use crate::shared::Shared;
use crate::tap::{HostEvent, Tap};

#[derive(Clone)]
pub struct HostCancel {
    shared: Arc<Shared>,
}

impl HostCancel {
    pub fn cancel(&self) {
        self.shared.cancel_all_for_host();
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct HostRun {
    pub events: Vec<HostEvent>,
    pub drain: DrainReport,
}

pub struct TestHost<F: Fields> {
    builder: RunnerBuilder,
    args: StartArgs<F>,
    tap: Arc<Tap>,
}

impl<F: Fields> TestHost<F> {
    pub fn new(
        client: JobsClient,
        settings: SettingsBuilder,
        args: StartArgs<F>,
    ) -> Result<Self, Error> {
        let settings = settings
            .max_concurrent_runs(1)
            .drain_timeout(Duration::MAX)
            .build()?;
        let tap = Arc::new(Tap::default());
        let builder = RunnerBuilder::new(client, settings).with_tap(Arc::clone(&tap));
        builder.shared().take_one_run_only();
        Ok(Self {
            builder,
            args: args.without_shutdown_and_signals(),
            tap,
        })
    }

    pub fn with_lifecycle_hook(mut self, hook: LifecycleHook) -> Self {
        self.builder = self.builder.with_lifecycle_hook(hook);
        self
    }

    pub fn with_queue_observer(mut self, observer: Arc<dyn QueueObserver>) -> Self {
        self.builder = self.builder.with_queue_observer(observer);
        self
    }

    pub fn instance_lookup(&self) -> InstanceLookup {
        self.builder.instance_lookup()
    }

    pub fn cancel_handle(&self) -> HostCancel {
        HostCancel {
            shared: self.builder.shared(),
        }
    }

    pub async fn run(self) -> Result<HostRun, Error> {
        let shared = self.builder.shared();
        let shutdown = CancellationToken::new();
        let start = tokio::spawn(
            self.builder
                .start(self.args.with_shutdown(shutdown.clone())),
        );
        let mut finished = shared.finished.subscribe();
        let _ = finished.wait_for(|count| *count >= 1).await;
        shutdown.cancel();
        let drain = start
            .await
            .map_err(|error| Error::new(ErrorCode::JobsFailed, error.to_string()))??;
        Ok(HostRun {
            events: self.tap.events(),
            drain,
        })
    }
}

pub async fn advance_clock(by: Duration) {
    tokio::time::advance(by).await;
}
