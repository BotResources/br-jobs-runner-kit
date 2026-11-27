use crate::cluster::{EventTarget, PoolEvent};
use crate::config::PoolConfig;
use crate::facts::{Mark, PodFacts, PoolState};
use crate::rules::{
    ClaimWindow, ScaleDownRules, condition_write, damp, desired_size, floor_now, judge_pool,
};

use super::{Engine, PoolId, pool_id};

impl Engine {
    fn pool_pods<'a>(
        &'a self,
        pool: &'a PoolConfig,
        state: PoolState,
    ) -> impl Iterator<Item = &'a PodFacts> {
        self.pods
            .values()
            .map(|tracked| &tracked.facts)
            .filter(move |pod| {
                pod.namespace == pool.namespace
                    && pod.pool_id.as_deref() == Some(pool.id.as_str())
                    && pod.pool_state == state
            })
    }

    pub(crate) async fn evaluate_all(&mut self) {
        let ids: Vec<PoolId> = self.config.pools.iter().map(pool_id).collect();
        for id in ids {
            self.evaluate_pool_id(&id).await;
        }
    }

    pub(crate) async fn evaluate_pool_id(&mut self, id: &PoolId) {
        let Some(pool) = self.config.pool(&id.0, &id.1).cloned() else {
            return;
        };
        self.evaluate_pool(&pool).await;
    }

    pub(super) fn refresh_pool_metrics(&self, pool: &PoolConfig) {
        let ready = self
            .pool_pods(pool, PoolState::Warm)
            .filter(|pod| pod.ready)
            .count();
        let idle = self
            .pool_pods(pool, PoolState::Claimed)
            .filter(|pod| pod.clean == Mark::Value(true))
            .count();
        self.metrics
            .set_pool_warm_ready(&pool.id, u32::try_from(ready).unwrap_or(u32::MAX));
        self.metrics
            .set_idle_pods(&pool.id, u32::try_from(idle).unwrap_or(u32::MAX));
    }

    async fn evaluate_pool(&mut self, pool: &PoolConfig) {
        if !self.synced.contains(&pool.namespace) {
            return;
        }
        self.refresh_pool_metrics(pool);
        if !self.leadership.is_leader() {
            return;
        }
        let id = pool_id(pool);
        let replica_set = match self
            .api
            .get_replica_set(&pool.namespace, &pool.replicaset)
            .await
        {
            Ok(Some(replica_set)) => replica_set,
            Ok(None) => {
                self.action_failed(
                    "the ReplicaSet of a pool is absent",
                    &pool.replicaset,
                    &"absent",
                );
                return;
            }
            Err(error) => {
                self.action_failed(
                    "the ReplicaSet of a pool could not be read",
                    &pool.replicaset,
                    &error,
                );
                let retry_at = self.clock.now().plus(self.retry_period());
                self.pool_wake.insert(id, retry_at);
                return;
            }
        };
        let now = self.clock.now();
        let floor = floor_now(pool, now, &self.config.timezone);
        let (window, bucket) = (self.config.rate_window, self.config.rate_bucket);
        let rate = self
            .claims
            .entry(id.clone())
            .or_insert_with(|| ClaimWindow::new(window, bucket))
            .peak(now);
        let desired = desired_size(rate, pool, floor);
        self.metrics.set_pool_desired(&pool.id, desired.clamped);
        self.metrics
            .set_claims_per_second(&pool.id, rate.per_second());
        let current = u32::try_from(replica_set.replicas.unwrap_or(0)).unwrap_or(0);
        let rules = ScaleDownRules {
            cooldown: self.config.scale_down_cooldown,
            step: self.config.scale_down_step,
            interval: self.config.scale_down_step_interval,
        };
        let damped = damp(
            self.scale.get(&id).copied().unwrap_or_default(),
            current,
            desired.clamped,
            now,
            &rules,
        );
        self.scale.insert(id.clone(), damped.state);
        match damped.wake_at {
            Some(at) => self.pool_wake.insert(id.clone(), at),
            None => self.pool_wake.remove(&id),
        };
        if damped.replicas != current && self.leadership.is_leader() {
            self.write_replicas(pool, &id, damped.replicas).await;
        }
        let target = EventTarget {
            name: pool.replicaset.clone(),
            uid: replica_set.uid.clone(),
        };
        self.at_max_event(pool, &id, &target, desired.above_max(pool), desired.raw)
            .await;
        let judged = judge_pool(self.pool_pods(pool, PoolState::Warm));
        if let Some(write) = condition_write(&replica_set.conditions, &judged, now)
            && self.leadership.is_leader()
            && let Err(error) = self
                .api
                .apply_condition(&pool.namespace, &pool.replicaset, &write)
                .await
        {
            self.action_failed(
                "the condition of a pool could not be written",
                &pool.replicaset,
                &error,
            );
            let retry_at = self.clock.now().plus(self.retry_period());
            self.pool_wake.insert(id.clone(), retry_at);
        }
        self.pending_events(pool, &id, &target).await;
    }

    async fn write_replicas(&mut self, pool: &PoolConfig, id: &PoolId, replicas: u32) {
        let value = i32::try_from(replicas).unwrap_or(i32::MAX);
        match self
            .api
            .apply_replicas(&pool.namespace, &pool.replicaset, value)
            .await
        {
            Ok(()) => tracing::info!(pool = pool.id, replicas, "the replicas of a pool were set"),
            Err(error) => {
                self.action_failed(
                    "the replicas of a pool could not be written",
                    &pool.replicaset,
                    &error,
                );
                let retry_at = self.clock.now().plus(self.retry_period());
                self.pool_wake.insert(id.clone(), retry_at);
            }
        }
    }

    async fn at_max_event(
        &mut self,
        pool: &PoolConfig,
        id: &PoolId,
        target: &EventTarget,
        above: bool,
        raw: u64,
    ) {
        if !above {
            self.at_max.remove(id);
            return;
        }
        if !self.leadership.is_leader() || !self.at_max.insert(id.clone()) {
            return;
        }
        let message = format!(
            "the size {raw} that the claim rate asks for is above max_replicas {}",
            pool.max_replicas
        );
        if let Err(error) = self
            .api
            .create_event(&pool.namespace, target, PoolEvent::AtMax, &message)
            .await
        {
            self.action_failed("an event could not be created", &pool.replicaset, &error);
            self.at_max.remove(id);
        }
    }

    async fn pending_events(&mut self, pool: &PoolConfig, id: &PoolId, target: &EventTarget) {
        let due: Vec<(String, String)> = self
            .pending_due
            .get(id)
            .map(|due| {
                due.iter()
                    .map(|(uid, name)| (uid.clone(), name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        for (uid, name) in due {
            if self.pending_notified.contains(&uid) || !self.leadership.is_leader() {
                continue;
            }
            let message = format!("the warm pod {name} is Pending and no container is failing");
            match self
                .api
                .create_event(
                    &pool.namespace,
                    target,
                    PoolEvent::WarmPodsPending,
                    &message,
                )
                .await
            {
                Ok(()) => {
                    self.pending_notified.insert(uid);
                }
                Err(error) => self.action_failed("an event could not be created", &name, &error),
            }
        }
    }
}
