use std::sync::Arc;

use br_runner_sandbox::Clock;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::bucket::{Bucket, BucketError, GuardedBucket};
use crate::cluster::{KubeApi, PoolApi, Views};
use crate::config::KeeperConfig;
use crate::controller::Engine;
use crate::error::KeeperError;
use crate::http::{HttpState, serve};
use crate::leader::{Elector, Leadership};
use crate::metrics::Metrics;
use crate::readiness::Readiness;
use crate::rules::SweepRules;
use crate::sweep::Sweeper;

pub struct Parts {
    api: Arc<dyn PoolApi>,
    bucket: Arc<dyn Bucket>,
    clock: Arc<dyn Clock>,
    identity: String,
    default_lease_namespace: String,
}

impl Parts {
    pub fn new(
        api: Arc<dyn PoolApi>,
        bucket: Arc<dyn Bucket>,
        clock: Arc<dyn Clock>,
        identity: String,
        default_lease_namespace: String,
    ) -> Self {
        Self {
            api,
            bucket,
            clock,
            identity,
            default_lease_namespace,
        }
    }
}

struct Shared {
    views: Views,
    config: Arc<KeeperConfig>,
    metrics: Arc<Metrics>,
    readiness: Arc<Readiness>,
    leadership: Arc<Leadership>,
    shutdown: CancellationToken,
}

type Tasks = JoinSet<&'static str>;

pub async fn run(
    config: KeeperConfig,
    parts: Parts,
    shutdown: CancellationToken,
) -> Result<(), KeeperError> {
    let listener = TcpListener::bind(config.listen_address)
        .await
        .map_err(|error| KeeperError::Listen {
            address: config.listen_address.to_string(),
            reason: error.to_string(),
        })?;
    run_on(listener, config, parts, shutdown).await
}

pub(crate) async fn run_on(
    listener: TcpListener,
    config: KeeperConfig,
    parts: Parts,
    shutdown: CancellationToken,
) -> Result<(), KeeperError> {
    let views = Views::new();
    let readiness = Arc::new(Readiness::starting());
    readiness.watch(views.clone());
    let shared = Shared {
        views,
        config: Arc::new(config),
        metrics: Arc::new(Metrics::new()),
        readiness,
        leadership: Leadership::new(parts.clock.clone()),
        shutdown,
    };
    let mut tasks: Tasks = JoinSet::new();
    spawn_http(&mut tasks, &shared, listener);
    let result = match start(&shared, &parts).await {
        Ok(()) => {
            spawn_controllers(&mut tasks, &shared, parts);
            wait(&mut tasks, &shared.shutdown).await
        }
        Err(error) => Err(error),
    };
    shared.shutdown.cancel();
    while tasks.join_next().await.is_some() {}
    result
}

fn spawn_http(tasks: &mut Tasks, shared: &Shared, listener: TcpListener) {
    let (metrics, leadership) = (shared.metrics.clone(), shared.leadership.clone());
    let state = Arc::new(HttpState {
        readiness: shared.readiness.clone(),
        metrics: Box::new(move || metrics.render(leadership.is_leader())),
    });
    let stop = shared.shutdown.clone();
    tasks.spawn(async move {
        serve(listener, state, stop).await;
        "http"
    });
}

async fn start(shared: &Shared, parts: &Parts) -> Result<(), KeeperError> {
    let config = &shared.config;
    parts
        .api
        .managed_namespaces(&config.managed_namespace_selector)
        .await
        .map_err(|error| KeeperError::ApiUnreachable(error.to_string()))?;
    match parts.bucket.check().await {
        Ok(()) => {}
        Err(BucketError::Absent) => {
            return Err(KeeperError::BucketAbsent(config.store.bucket.clone()));
        }
        Err(error) => return Err(KeeperError::StoreUnreachable(error.to_string())),
    }
    shared.readiness.store_ready();
    Ok(())
}

fn sweeper_of(shared: &Shared, bucket: Arc<dyn Bucket>, clock: Arc<dyn Clock>) -> Sweeper {
    let config = &shared.config;
    Sweeper {
        bucket: Arc::new(GuardedBucket::new(bucket)),
        clock,
        leadership: shared.leadership.clone(),
        metrics: shared.metrics.clone(),
        rules: SweepRules::from(config.as_ref()),
        page_size: config.sweep_page_size,
        interval: config.sweep_interval,
    }
}

fn spawn_controllers(tasks: &mut Tasks, shared: &Shared, parts: Parts) {
    let config = &shared.config;
    let notices = shared.views.notices();
    let views = shared.views.clone();
    let (api, selector) = (parts.api.clone(), config.managed_namespace_selector.clone());
    let stop = shared.shutdown.clone();
    tasks.spawn(async move {
        views.run(api, selector, stop).await;
        "views"
    });
    let engine = Engine::new(
        parts.api.clone(),
        shared.views.clone(),
        parts.clock.clone(),
        shared.leadership.clone(),
        shared.metrics.clone(),
        config.clone(),
    );
    let stop = shared.shutdown.clone();
    tasks.spawn(async move {
        engine.run(notices, stop).await;
        "controller"
    });
    let kube: Arc<dyn KubeApi> = parts.api;
    let elector = Elector {
        api: kube,
        clock: parts.clock.clone(),
        leadership: shared.leadership.clone(),
        namespace: config
            .lease
            .namespace
            .clone()
            .unwrap_or(parts.default_lease_namespace),
        identity: parts.identity,
        lease: config.lease.clone(),
        metrics: shared.metrics.clone(),
    };
    let stop = shared.shutdown.clone();
    tasks.spawn(async move {
        elector.run(stop).await;
        "elector"
    });
    let sweeper = sweeper_of(shared, parts.bucket, parts.clock);
    let stop = shared.shutdown.clone();
    tasks.spawn(async move {
        sweeper.run(stop).await;
        "sweeper"
    });
}

async fn wait(tasks: &mut Tasks, shutdown: &CancellationToken) -> Result<(), KeeperError> {
    tokio::select! {
        () = shutdown.cancelled() => Ok(()),
        ended = tasks.join_next() => match ended {
            None => Ok(()),
            Some(Ok(_)) if shutdown.is_cancelled() => Ok(()),
            Some(Ok(name)) => Err(KeeperError::TaskEnded(name)),
            Some(Err(_)) => Err(KeeperError::TaskEnded("panicked")),
        },
    }
}

#[cfg(test)]
mod tests;
