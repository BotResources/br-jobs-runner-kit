mod condition;
mod events;
mod expiry;
mod observation;
mod reconcile;
mod resource;
mod rollout;
mod serving;
mod sizing;
mod status;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use br_runner_sandbox::{Clock, Timestamp};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_util::sync::CancellationToken;

use crate::cluster::{Notice, Observer, PoolApi, PoolWriter, Views};
use crate::config::KeeperConfig;
use crate::facts::PodFacts;
use crate::leader::Leadership;
use crate::metrics::Metrics;
use crate::rules::{ClaimWindow, ScaleState};
use crate::time::{earliest, until};

pub type PodId = (String, String);

pub(crate) struct Tracked {
    pub facts: PodFacts,
    pub failed_since: Option<Timestamp>,
    pub recheck_at: Option<Timestamp>,
}

#[derive(Default)]
pub(crate) struct Dirty {
    pub all: bool,
    pub namespaces: BTreeSet<String>,
}

pub struct Engine {
    pub(crate) api: Arc<dyn PoolApi>,
    pub(crate) views: Views,
    pub(crate) observer: Observer,
    pub(crate) writer: PoolWriter,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) leadership: Arc<Leadership>,
    pub(crate) metrics: Arc<Metrics>,
    pub(crate) config: Arc<KeeperConfig>,
    pub(crate) pods: HashMap<PodId, Tracked>,
    pub(crate) claims: HashMap<String, ClaimWindow>,
    pub(crate) scale: HashMap<String, ScaleState>,
    pub(crate) pool_wake: HashMap<String, Timestamp>,
    pub(crate) status_written: HashMap<(String, String), Timestamp>,
    pub(crate) at_max: HashSet<String>,
    pub(crate) pending_due: HashMap<String, HashMap<String, String>>,
    pub(crate) pending_notified: HashSet<String>,
    pub(crate) unreadable: HashSet<String>,
    pub(crate) unhashed: HashMap<String, HashSet<String>>,
    pub(crate) synced: HashSet<String>,
    pub(crate) seen: HashSet<String>,
    pub(crate) next_tick: Timestamp,
}

impl Engine {
    pub fn new(
        api: Arc<dyn PoolApi>,
        views: Views,
        clock: Arc<dyn Clock>,
        leadership: Arc<Leadership>,
        metrics: Arc<Metrics>,
        config: Arc<KeeperConfig>,
    ) -> Self {
        let next_tick = clock.now();
        Self {
            observer: Observer::new(api.clone(), views.clone()),
            writer: PoolWriter::new(api.clone(), metrics.clone(), config.pod_keys()),
            api,
            views,
            clock,
            leadership,
            metrics,
            config,
            pods: HashMap::new(),
            claims: HashMap::new(),
            scale: HashMap::new(),
            pool_wake: HashMap::new(),
            status_written: HashMap::new(),
            at_max: HashSet::new(),
            pending_due: HashMap::new(),
            pending_notified: HashSet::new(),
            unreadable: HashSet::new(),
            unhashed: HashMap::new(),
            synced: HashSet::new(),
            seen: HashSet::new(),
            next_tick,
        }
    }

    pub(crate) fn retry_period(&self) -> Duration {
        self.config.lease.retry_period
    }

    pub(crate) fn action_failed(&self, what: &str, subject: &str, error: &dyn std::fmt::Display) {
        tracing::warn!(%error, subject, "{what}");
        self.metrics.sweep_error();
    }

    pub(crate) fn wake_by(&mut self, namespace: &str, at: Timestamp) {
        self.pool_wake
            .entry(namespace.to_owned())
            .and_modify(|held| *held = (*held).min(at))
            .or_insert(at);
    }

    pub(crate) fn retry_soon(&mut self, namespace: &str) {
        let at = self.clock.now().plus(self.retry_period());
        self.wake_by(namespace, at);
    }

    fn next_wake(&self) -> Timestamp {
        let pods = self.pods.values().filter_map(|tracked| tracked.recheck_at);
        let pools = self.pool_wake.values().copied();
        pods.chain(pools)
            .chain([self.next_tick])
            .fold(None, |best, candidate| earliest(best, Some(candidate)))
            .unwrap_or(self.next_tick)
    }

    #[cfg(test)]
    pub async fn notice(&mut self, notice: Notice) {
        let mut dirty = Dirty::default();
        self.ingest(notice, &mut dirty).await;
        self.settle(dirty).await;
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
        if self.next_tick.has_passed(now) {
            self.next_tick = now.plus(self.config.rate_bucket);
            self.reconcile_all().await;
            return;
        }
        let woken: Vec<String> = self
            .pool_wake
            .iter()
            .filter(|(_, at)| at.has_passed(now))
            .map(|(namespace, _)| namespace.clone())
            .collect();
        for namespace in woken {
            self.reconcile(&namespace).await;
        }
    }

    pub async fn gained_leadership(&mut self) {
        let ids: Vec<PodId> = self.pods.keys().cloned().collect();
        for id in ids {
            self.judge(&id).await;
        }
        self.reconcile_all().await;
    }

    pub async fn run(
        mut self,
        mut notices: UnboundedReceiver<Notice>,
        shutdown: CancellationToken,
    ) {
        loop {
            let pause = until(self.clock.now(), self.next_wake());
            tokio::select! {
                () = shutdown.cancelled() => return,
                notice = notices.recv() => {
                    let Some(first) = notice else { return };
                    let mut dirty = Dirty::default();
                    self.ingest(first, &mut dirty).await;
                    while let Ok(more) = notices.try_recv() {
                        self.ingest(more, &mut dirty).await;
                    }
                    self.settle(dirty).await;
                }
                () = self.leadership.gained() => self.gained_leadership().await,
                () = tokio::time::sleep(pause) => self.wake().await,
            }
        }
    }
}

#[cfg(test)]
mod tests;
