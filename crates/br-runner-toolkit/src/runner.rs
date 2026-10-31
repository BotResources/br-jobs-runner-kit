use std::num::NonZeroUsize;
use std::sync::Arc;

use br_graph_contract::{Fields, Graph, State};
use br_runner_jobs::{Capacity, Content, JobsClient, QueueObserver};
use tokio_util::sync::CancellationToken;

use crate::calls::Calls;
use crate::drain::{DrainReport, drain};
use crate::error::{Error, ErrorCode};
use crate::failure::FailureMapping;
use crate::hook::LifecycleHook;
use crate::instances::{InstanceLookup, Instances};
use crate::plan::{AnswerFn, Plan};
use crate::run_start::RunStart;
use crate::settings::Settings;
use crate::shared::Shared;
#[cfg(feature = "test-host")]
use crate::tap::Tap;
use crate::{feeds, presence, slot};

pub struct StartArgs<F: Fields> {
    plan: Plan<F>,
    shutdown: Option<CancellationToken>,
    process_signals: bool,
}

impl<F: Fields> StartArgs<F> {
    pub fn new(graph: Arc<Graph<F>>, run_start: Arc<dyn RunStart<F>>, can_restart: bool) -> Self {
        Self {
            plan: Plan {
                graph,
                run_start,
                can_restart,
                state_limit: None,
                mapping: None,
                answer: None,
            },
            shutdown: None,
            process_signals: false,
        }
    }

    #[cfg(feature = "test-host")]
    pub(crate) fn without_shutdown_and_signals(mut self) -> Self {
        self.shutdown = None;
        self.process_signals = false;
        self
    }

    pub fn with_shutdown(mut self, shutdown: CancellationToken) -> Self {
        self.shutdown = Some(shutdown);
        self
    }

    pub fn with_process_signals(mut self) -> Self {
        self.process_signals = true;
        self
    }

    pub fn with_state_limit(mut self, limit: NonZeroUsize) -> Self {
        self.plan.state_limit = Some(limit);
        self
    }

    pub fn with_failure_mapping(mut self, mapping: Arc<dyn FailureMapping>) -> Self {
        self.plan.mapping = Some(mapping);
        self
    }

    pub fn with_answer<A>(mut self, answer: A) -> Self
    where
        A: Fn(&State<F>) -> Result<Content, String> + Send + Sync + 'static,
    {
        let answer: AnswerFn<F> = Arc::new(answer);
        self.plan.answer = Some(answer);
        self
    }
}

pub struct RunnerBuilder {
    shared: Arc<Shared>,
    instances: Arc<Instances>,
}

impl RunnerBuilder {
    pub fn new(client: JobsClient, settings: Settings) -> Self {
        let instances = Arc::new(Instances::default());
        Self {
            shared: Shared::new(
                Calls::new(client, None),
                settings,
                None,
                None,
                Arc::clone(&instances),
            ),
            instances,
        }
    }

    fn edit(mut self, change: impl FnOnce(&mut Shared)) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            change(shared);
        }
        self
    }

    pub fn with_lifecycle_hook(self, hook: LifecycleHook) -> Self {
        self.edit(|shared| shared.hook = Some(hook))
    }

    pub fn with_queue_observer(self, observer: Arc<dyn QueueObserver>) -> Self {
        self.edit(|shared| shared.author_observer = Some(observer))
    }

    #[cfg(feature = "test-host")]
    pub(crate) fn with_tap(self, tap: Arc<Tap>) -> Self {
        self.edit(|shared| {
            shared.calls = Calls::new(shared.calls.client().clone(), Some(tap));
        })
    }

    #[cfg(feature = "test-host")]
    pub(crate) fn shared(&self) -> Arc<Shared> {
        Arc::clone(&self.shared)
    }

    pub fn instance_lookup(&self) -> InstanceLookup {
        InstanceLookup::new(Arc::clone(&self.instances))
    }

    pub async fn start<F: Fields>(self, args: StartArgs<F>) -> Result<DrainReport, Error> {
        let shared = self.shared;
        let ttl = shared
            .calls
            .client()
            .presence_time_to_live()
            .await
            .map_err(|error| Error::jobs(&error))?;
        let capacity = Capacity::new(shared.settings.max_concurrent_runs())
            .map_err(|error| Error::new(ErrorCode::SettingInvalid, error.to_string()))?;
        self.instances.mark_started();
        let plan = Arc::new(args.plan);
        let background = [
            tokio::spawn(presence::keep(Arc::clone(&shared), ttl, capacity)),
            tokio::spawn(feeds::dispatch_mode(Arc::clone(&shared))),
            tokio::spawn(feeds::cancel_orders(Arc::clone(&shared))),
        ];
        let slots = (0..shared.settings.max_concurrent_runs())
            .map(|_| tokio::spawn(slot::run(Arc::clone(&shared), Arc::clone(&plan))))
            .collect();
        wait_for_shutdown(args.shutdown, args.process_signals).await;
        let report = drain(&shared, slots).await;
        for task in &background {
            task.abort();
        }
        Ok(report)
    }
}

async fn wait_for_shutdown(shutdown: Option<CancellationToken>, process_signals: bool) {
    let author = async {
        match &shutdown {
            Some(token) => token.cancelled().await,
            None => std::future::pending().await,
        }
    };
    let signals = async {
        if process_signals {
            process_signal().await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        () = author => {}
        () = signals => {}
    }
}

#[cfg(unix)]
async fn process_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut terminate), Ok(mut interrupt)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        tracing::error!("the process signals cannot be listened to");
        return std::future::pending().await;
    };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
    }
}

#[cfg(not(unix))]
async fn process_signal() {
    if tokio::signal::ctrl_c().await.is_err() {
        tracing::error!("the process signals cannot be listened to");
        std::future::pending::<()>().await;
    }
}
