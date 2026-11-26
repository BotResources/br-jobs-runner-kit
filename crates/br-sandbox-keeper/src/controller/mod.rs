mod events;
mod expiry;
mod pool;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use br_runner_sandbox::{Clock, Timestamp};
use futures::StreamExt;
use futures::stream::{BoxStream, SelectAll};
use tokio_util::sync::CancellationToken;

use crate::cluster::{KubeApi, PodEvent};
use crate::config::{KeeperConfig, PoolConfig};
use crate::facts::PodFacts;
use crate::leader::Leadership;
use crate::metrics::Metrics;
use crate::readiness::Readiness;
use crate::rules::{ClaimWindow, ScaleState};
use crate::time::{earliest, until};

pub type PoolId = (String, String);
pub type PodId = (String, String);
pub type NamespacedEvents = SelectAll<BoxStream<'static, (String, PodEvent)>>;

pub(crate) struct Tracked {
    pub facts: PodFacts,
    pub failed_since: Option<Timestamp>,
    pub recheck_at: Option<Timestamp>,
}

pub struct Engine {
    pub(crate) api: Arc<dyn KubeApi>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) leadership: Arc<Leadership>,
    pub(crate) metrics: Arc<Metrics>,
    pub(crate) readiness: Arc<Readiness>,
    pub(crate) config: Arc<KeeperConfig>,
    pub(crate) pods: HashMap<PodId, Tracked>,
    pub(crate) claims: HashMap<PoolId, ClaimWindow>,
    pub(crate) scale: HashMap<PoolId, ScaleState>,
    pub(crate) pool_wake: HashMap<PoolId, Timestamp>,
    pub(crate) at_max: HashSet<PoolId>,
    pub(crate) pending_due: HashMap<PoolId, HashMap<String, String>>,
    pub(crate) pending_notified: HashSet<String>,
    pub(crate) unreadable: HashSet<String>,
    pub(crate) next_tick: Timestamp,
}

pub fn pool_id(pool: &PoolConfig) -> PoolId {
    (pool.namespace.clone(), pool.id.clone())
}

impl Engine {
    pub fn new(
        api: Arc<dyn KubeApi>,
        clock: Arc<dyn Clock>,
        leadership: Arc<Leadership>,
        metrics: Arc<Metrics>,
        readiness: Arc<Readiness>,
        config: Arc<KeeperConfig>,
    ) -> Self {
        let next_tick = clock.now();
        Self {
            api,
            clock,
            leadership,
            metrics,
            readiness,
            config,
            pods: HashMap::new(),
            claims: HashMap::new(),
            scale: HashMap::new(),
            pool_wake: HashMap::new(),
            at_max: HashSet::new(),
            pending_due: HashMap::new(),
            pending_notified: HashSet::new(),
            unreadable: HashSet::new(),
            next_tick,
        }
    }

    pub fn watches(api: &dyn KubeApi, namespaces: &[String]) -> NamespacedEvents {
        futures::stream::select_all(namespaces.iter().map(|namespace| {
            let owned = namespace.clone();
            api.watch_pods(namespace)
                .map(move |event| (owned.clone(), event))
                .boxed()
        }))
    }

    pub(crate) fn retry_period(&self) -> Duration {
        self.config.lease.retry_period
    }

    pub(crate) fn action_failed(&self, what: &str, subject: &str, error: &dyn std::fmt::Display) {
        tracing::warn!(%error, subject, "{what}");
        self.metrics.sweep_error();
    }

    fn next_wake(&self) -> Timestamp {
        let pods = self.pods.values().filter_map(|tracked| tracked.recheck_at);
        let pools = self.pool_wake.values().copied();
        pods.chain(pools)
            .chain(std::iter::once(self.next_tick))
            .fold(None, |best, candidate| earliest(best, Some(candidate)))
            .unwrap_or(self.next_tick)
    }

    pub async fn wake(&mut self) {
        let now = self.clock.now();
        let due: Vec<PodId> = self
            .pods
            .iter()
            .filter(|(_, tracked)| tracked.recheck_at.is_some_and(|at| at.has_passed(now)))
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            self.judge(&id).await;
        }
        let tick_due = self.next_tick.has_passed(now);
        let woken: Vec<PoolId> = self
            .pool_wake
            .iter()
            .filter(|(_, at)| at.has_passed(now))
            .map(|(id, _)| id.clone())
            .collect();
        if tick_due {
            self.next_tick = now.plus(self.config.rate_bucket);
            self.evaluate_all().await;
        } else {
            for id in woken {
                self.evaluate_pool_id(&id).await;
            }
        }
    }

    pub async fn gained_leadership(&mut self) {
        let ids: Vec<PodId> = self.pods.keys().cloned().collect();
        for id in ids {
            self.judge(&id).await;
        }
        self.evaluate_all().await;
    }

    pub async fn run(mut self, mut events: NamespacedEvents, shutdown: CancellationToken) {
        loop {
            let pause = until(self.clock.now(), self.next_wake());
            tokio::select! {
                () = shutdown.cancelled() => return,
                event = events.next() => match event {
                    Some((namespace, event)) => self.handle(&namespace, event).await,
                    None => return,
                },
                () = self.leadership.gained() => self.gained_leadership().await,
                () = tokio::time::sleep(pause) => self.wake().await,
            }
        }
    }
}

#[cfg(test)]
mod tests;
