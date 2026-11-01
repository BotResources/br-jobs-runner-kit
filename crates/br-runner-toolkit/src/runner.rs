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
#[cfg(feature = "test-host")]
use crate::host_state::HostState;
use crate::instances::{InstanceLookup, Instances};
use crate::plan::{AnswerFn, Plan};
use crate::run_start::RunStart;
use crate::settings::Settings;
use crate::shared::Shared;
use crate::signals::Signals;
use crate::spawn::{LoopBodies, LoopTasks, spawn_loop};
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
    client: JobsClient,
    settings: Settings,
    hook: Option<LifecycleHook>,
    author_observer: Option<Arc<dyn QueueObserver>>,
    instances: Arc<Instances>,
    #[cfg(feature = "test-host")]
    host: Option<Arc<HostState>>,
}

impl RunnerBuilder {
    pub fn new(client: JobsClient, settings: Settings) -> Self {
        Self {
            client,
            settings,
            hook: None,
            author_observer: None,
            instances: Arc::new(Instances::default()),
            #[cfg(feature = "test-host")]
            host: None,
        }
    }

    pub fn with_lifecycle_hook(mut self, hook: LifecycleHook) -> Self {
        self.hook = Some(hook);
        self
    }

    pub fn with_queue_observer(mut self, observer: Arc<dyn QueueObserver>) -> Self {
        self.author_observer = Some(observer);
        self
    }

    #[cfg(feature = "test-host")]
    pub(crate) fn with_host(mut self, host: Arc<HostState>) -> Self {
        self.host = Some(host);
        self
    }

    pub fn instance_lookup(&self) -> InstanceLookup {
        InstanceLookup::new(Arc::clone(&self.instances))
    }

    fn shared(self) -> (Arc<Shared>, Arc<Instances>) {
        #[cfg(feature = "test-host")]
        let tap = self.host.as_ref().map(|host| Arc::clone(&host.tap));
        #[cfg(not(feature = "test-host"))]
        let tap = None;
        let shared = Shared::new(
            Calls::new(self.client, tap),
            self.settings,
            self.hook,
            self.author_observer,
            Arc::clone(&self.instances),
        );
        #[cfg(feature = "test-host")]
        let shared = shared.with_host(self.host);
        (Arc::new(shared), self.instances)
    }

    pub async fn start<F: Fields>(self, args: StartArgs<F>) -> Result<DrainReport, Error> {
        let mut signals = if args.process_signals {
            Some(Signals::install()?)
        } else {
            None
        };
        let (shared, instances) = self.shared();
        let ttl = shared
            .calls
            .client()
            .presence_time_to_live()
            .await
            .map_err(|error| Error::jobs(&error))?;
        let capacity = Capacity::new(shared.settings.max_concurrent_runs())
            .map_err(|error| Error::new(ErrorCode::SettingInvalid, error.to_string()))?;
        instances.mark_started();
        let plan = Arc::new(args.plan);
        let LoopTasks {
            presence,
            feeds,
            slots,
        } = spawn_loop(
            &shared,
            shared.settings.max_concurrent_runs(),
            LoopBodies {
                presence: {
                    let shared = Arc::clone(&shared);
                    move || presence::keep(Arc::clone(&shared), ttl, capacity)
                },
                dispatch_mode: {
                    let shared = Arc::clone(&shared);
                    move || feeds::dispatch_mode(Arc::clone(&shared))
                },
                cancel_orders: {
                    let shared = Arc::clone(&shared);
                    move || feeds::cancel_orders(Arc::clone(&shared))
                },
                slot: {
                    let (shared, plan) = (Arc::clone(&shared), Arc::clone(&plan));
                    move || slot::run(Arc::clone(&shared), Arc::clone(&plan))
                },
            },
        );
        #[cfg(feature = "test-host")]
        let host_cancel = shared.host.as_ref().map(|host| {
            let (shared, mut cancel) = (Arc::clone(&shared), host.cancel.subscribe());
            tokio::spawn(async move {
                if cancel.wait_for(|cancelled| *cancelled).await.is_ok() {
                    shared.cancel_all_runs();
                }
            })
        });
        wait_for_shutdown(args.shutdown, signals.as_mut()).await;
        let report = drain(&shared, slots).await;
        presence.abort();
        let _ = presence.await;
        for task in &feeds {
            task.abort();
        }
        #[cfg(feature = "test-host")]
        if let Some(task) = host_cancel {
            task.abort();
        }
        if let Err(error) = shared.calls.delete_presence().await {
            tracing::error!(%error, "the presence could not be deleted, the time to live of the bucket ends it");
        }
        Ok(report)
    }
}

async fn wait_for_shutdown(shutdown: Option<CancellationToken>, signals: Option<&mut Signals>) {
    let author = async {
        match &shutdown {
            Some(token) => token.cancelled().await,
            None => std::future::pending().await,
        }
    };
    let process = async {
        match signals {
            Some(signals) => signals.received().await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        () = author => {}
        () = process => {}
    }
}
