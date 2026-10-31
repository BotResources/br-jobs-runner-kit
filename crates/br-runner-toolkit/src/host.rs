use std::time::Duration;

use br_graph_contract::Fields;
use br_runner_jobs::{JobsClient, QueueObserver};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::drain::DrainReport;
use crate::error::{Error, ErrorCode};
use crate::hook::LifecycleHook;
use crate::host_state::HostState;
use crate::instances::InstanceLookup;
use crate::runner::{RunnerBuilder, StartArgs};
use crate::settings::SettingsBuilder;
use crate::tap::HostEvent;

#[derive(Clone)]
pub struct HostCancel {
    state: Arc<HostState>,
}

impl HostCancel {
    pub fn cancel(&self) {
        self.state.cancel.send_replace(true);
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
    state: Arc<HostState>,
}

fn run_ended(
    ended: Result<Result<DrainReport, Error>, tokio::task::JoinError>,
) -> Result<DrainReport, Error> {
    match ended {
        Ok(result) => result,
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => Err(Error::new(ErrorCode::TaskEnded, error.to_string())),
    }
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
        let state = HostState::new();
        Ok(Self {
            builder: RunnerBuilder::new(client, settings).with_host(Arc::clone(&state)),
            args: args.without_shutdown_and_signals(),
            state,
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
            state: Arc::clone(&self.state),
        }
    }

    pub async fn run(self) -> Result<HostRun, Error> {
        let state = self.state;
        let shutdown = CancellationToken::new();
        let mut start = tokio::spawn(
            self.builder
                .start(self.args.with_shutdown(shutdown.clone())),
        );
        let mut finished = state.finished.subscribe();
        let first_run_forgotten = async {
            let _ = finished.wait_for(|count| *count >= 1).await;
        };
        let drain = tokio::select! {
            () = first_run_forgotten => {
                shutdown.cancel();
                run_ended(start.await)?
            }
            ended = &mut start => run_ended(ended)?,
        };
        Ok(HostRun {
            events: state.tap.events(),
            drain,
        })
    }
}
