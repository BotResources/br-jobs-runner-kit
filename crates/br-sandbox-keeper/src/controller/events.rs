use std::collections::HashSet;

use crate::cluster::{Notice, ViewEvent, Watched};
use crate::facts::{Mark, Phase, PodFacts, PoolState};
use crate::rules::{ClaimWindow, pod_failure};
use crate::time::elapsed;

use super::{Dirty, Engine, PodId, Tracked};

pub(super) fn pool_of(pod: &PodFacts) -> Option<String> {
    (pod.pool_id.as_deref() == Some(pod.namespace.as_str())).then(|| pod.namespace.clone())
}

impl Engine {
    pub(super) async fn ingest(&mut self, notice: Notice, dirty: &mut Dirty) {
        match notice {
            Notice::Pods { namespace, event } => self.pod_event(&namespace, *event, dirty).await,
            Notice::Resource {
                watched,
                namespace,
                failure,
            } => {
                if let Some(reason) = failure {
                    tracing::error!(?watched, namespace, reason, "a watch failed");
                    self.metrics.sweep_error();
                }
                match watched {
                    Watched::Pools => {
                        dirty.namespaces.insert(namespace);
                    }
                    Watched::Namespaces | Watched::Bindings => {
                        self.release_unmanaged();
                        dirty.all = true;
                    }
                }
            }
        }
    }

    pub(super) async fn settle(&mut self, dirty: Dirty) {
        self.refresh_fleet_metrics();
        if dirty.all {
            self.reconcile_all().await;
            return;
        }
        for namespace in dirty.namespaces {
            self.reconcile(&namespace).await;
        }
    }

    async fn pod_event(&mut self, namespace: &str, event: ViewEvent<PodFacts>, dirty: &mut Dirty) {
        let mut touched: HashSet<String> = HashSet::new();
        match event {
            ViewEvent::Snapshot(pods) => {
                let ids = self.snapshot(namespace, pods, &mut touched);
                for id in ids {
                    self.judge(&id).await;
                }
                self.synced.insert(namespace.to_owned());
                touched.insert(namespace.to_owned());
            }
            ViewEvent::Applied(pod) => {
                if let Some(id) = self.apply(pod, true, &mut touched) {
                    self.judge(&id).await;
                }
            }
            ViewEvent::Deleted { key } => {
                self.remove(&(namespace.to_owned(), key), &mut touched);
            }
            ViewEvent::Failed { reason } | ViewEvent::Missing { reason } => {
                tracing::error!(namespace, reason, "the pod watch of a namespace failed");
                self.metrics.sweep_error();
                self.synced.remove(namespace);
                touched.insert(namespace.to_owned());
            }
        }
        dirty.namespaces.extend(touched);
    }

    fn snapshot(
        &mut self,
        namespace: &str,
        pods: Vec<PodFacts>,
        touched: &mut HashSet<String>,
    ) -> Vec<PodId> {
        let incoming: HashSet<String> = pods.iter().map(|pod| pod.name.clone()).collect();
        let gone: Vec<PodId> = self
            .pods
            .keys()
            .filter(|(ns, name)| ns == namespace && !incoming.contains(name))
            .cloned()
            .collect();
        for id in gone {
            self.remove(&id, touched);
        }
        pods.into_iter()
            .filter_map(|pod| self.apply(pod, false, touched))
            .collect()
    }

    pub(crate) fn apply(
        &mut self,
        mut pod: PodFacts,
        count_unseen: bool,
        touched: &mut HashSet<String>,
    ) -> Option<PodId> {
        let now = self.clock.now();
        let id: PodId = (pod.namespace.clone(), pod.name.clone());
        let previous = self.pods.get(&id);
        pod.been_ready =
            pod.been_ready || pod.ready || previous.is_some_and(|tracked| tracked.facts.been_ready);
        let pool = pool_of(&pod);
        let was_warm = previous.is_some_and(|tracked| tracked.facts.pool_state == PoolState::Warm);
        let unseen = previous.is_none();
        let changed = previous.is_none_or(|tracked| {
            pod_failure(&tracked.facts) != pod_failure(&pod)
                || tracked.facts.pool_state != pod.pool_state
                || tracked.facts.ready != pod.ready
                || tracked.facts.template_hash != pod.template_hash
                || tracked.facts.phase != pod.phase
        });
        let cold_claim = unseen
            && count_unseen
            && pod.pool_state == PoolState::Claimed
            && !pod.owned
            && elapsed(now, pod.created_at) <= self.config.rate_window;
        let warm_to_claimed = was_warm && pod.pool_state == PoolState::Claimed;
        if let Some(pool) = pool {
            if warm_to_claimed || cold_claim {
                let (window, bucket) = (self.config.rate_window, self.config.rate_bucket);
                self.claims
                    .entry(pool.clone())
                    .or_insert_with(|| ClaimWindow::new(window, bucket))
                    .record(now);
                touched.insert(pool.clone());
            }
            if changed {
                touched.insert(pool);
            }
        }
        let failed_since = if pod.phase == Phase::Failed {
            Some(
                previous
                    .and_then(|tracked| tracked.failed_since)
                    .unwrap_or(now),
            )
        } else {
            None
        };
        let recheck_at = previous.and_then(|tracked| tracked.recheck_at);
        self.pods.insert(
            id.clone(),
            Tracked {
                facts: pod,
                failed_since,
                recheck_at: recheck_at.or(Some(now)),
            },
        );
        Some(id)
    }

    pub(crate) fn remove(&mut self, id: &PodId, touched: &mut HashSet<String>) {
        let Some(tracked) = self.pods.remove(id) else {
            return;
        };
        self.unreadable.remove(&tracked.facts.uid);
        self.pending_notified.remove(&tracked.facts.uid);
        if let Some(unhashed) = self.unhashed.get_mut(&id.0) {
            unhashed.remove(&tracked.facts.uid);
        }
        if let Some(pool) = pool_of(&tracked.facts) {
            if let Some(due) = self.pending_due.get_mut(&pool) {
                due.remove(&tracked.facts.uid);
            }
            touched.insert(pool);
        }
    }

    pub(crate) fn refresh_fleet_metrics(&self) {
        let idle = self
            .pods
            .values()
            .filter(|tracked| {
                tracked.facts.pool_state == PoolState::Claimed
                    && tracked.facts.clean == Mark::Value(true)
            })
            .count();
        self.metrics.set_idle_pods(idle);
        let unhashed: HashSet<&String> = self.unhashed.values().flatten().collect();
        let unreadable: HashSet<&String> = self.unreadable.iter().collect();
        self.metrics
            .set_unreadable_pods(unhashed.union(&unreadable).count());
    }
}
