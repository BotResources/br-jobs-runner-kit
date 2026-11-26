use std::collections::HashSet;

use crate::cluster::PodEvent;
use crate::facts::{Phase, PodFacts, PoolState};
use crate::rules::{ClaimWindow, pod_failure};
use crate::time::elapsed;

use super::{Engine, PodId, PoolId, Tracked};

fn pool_of(pod: &PodFacts) -> Option<PoolId> {
    pod.pool_id
        .as_ref()
        .map(|id| (pod.namespace.clone(), id.clone()))
}

impl Engine {
    pub async fn handle(&mut self, namespace: &str, event: PodEvent) {
        let mut touched: HashSet<PoolId> = HashSet::new();
        match event {
            PodEvent::Snapshot(pods) => {
                let ids = self.snapshot(namespace, pods, &mut touched);
                for id in ids {
                    self.judge(&id).await;
                }
                self.readiness.namespace_synced(namespace);
            }
            PodEvent::Applied(pod) => {
                let id = self.apply(*pod, true, &mut touched);
                if let Some(id) = id {
                    self.judge(&id).await;
                }
            }
            PodEvent::Deleted { name } => self.remove(&(namespace.to_owned(), name), &mut touched),
        }
        self.refresh_unreadable_metric();
        for pool in touched {
            self.evaluate_pool_id(&pool).await;
        }
    }

    fn snapshot(
        &mut self,
        namespace: &str,
        pods: Vec<PodFacts>,
        touched: &mut HashSet<PoolId>,
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
        for pool in self
            .config
            .pools
            .iter()
            .filter(|pool| pool.namespace == namespace)
        {
            touched.insert(super::pool_id(pool));
        }
        pods.into_iter()
            .filter_map(|pod| self.apply(pod, false, touched))
            .collect()
    }

    pub(crate) fn apply(
        &mut self,
        pod: PodFacts,
        count_unseen: bool,
        touched: &mut HashSet<PoolId>,
    ) -> Option<PodId> {
        let pool = pool_of(&pod)?;
        self.config.pool(&pool.0, &pool.1)?;
        let now = self.clock.now();
        let id: PodId = (pod.namespace.clone(), pod.name.clone());
        let previous = self.pods.get(&id);
        let was_warm = previous.is_some_and(|tracked| tracked.facts.pool_state == PoolState::Warm);
        let unseen = previous.is_none();
        let changed_failure = previous.is_none_or(|tracked| {
            pod_failure(&tracked.facts) != pod_failure(&pod)
                || tracked.facts.pool_state != pod.pool_state
                || tracked.facts.ready != pod.ready
        });
        let cold_claim = unseen
            && count_unseen
            && pod.pool_state == PoolState::Claimed
            && !pod.owned
            && elapsed(now, pod.created_at) <= self.config.rate_window;
        let warm_to_claimed = was_warm && pod.pool_state == PoolState::Claimed;
        if warm_to_claimed || cold_claim {
            let (window, bucket) = (self.config.rate_window, self.config.rate_bucket);
            self.claims
                .entry(pool.clone())
                .or_insert_with(|| ClaimWindow::new(window, bucket))
                .record(now);
            touched.insert(pool.clone());
        }
        if changed_failure {
            touched.insert(pool);
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

    pub(crate) fn remove(&mut self, id: &PodId, touched: &mut HashSet<PoolId>) {
        let Some(tracked) = self.pods.remove(id) else {
            return;
        };
        self.unreadable.remove(&tracked.facts.uid);
        self.pending_notified.remove(&tracked.facts.uid);
        if let Some(pool) = pool_of(&tracked.facts) {
            if let Some(due) = self.pending_due.get_mut(&pool) {
                due.remove(&tracked.facts.uid);
            }
            touched.insert(pool);
        }
    }

    pub(crate) fn refresh_unreadable_metric(&self) {
        self.metrics.set_unreadable_pods(self.unreadable.len());
    }
}
