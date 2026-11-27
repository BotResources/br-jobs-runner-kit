use crate::cluster::{EventTarget, PoolEvent};
use crate::pool::{Owner, PoolSettings, replicaset_name};
use crate::rules::{ClaimWindow, Desired, ScaleDownRules, damp, desired_size, floor_now};

use super::Engine;

impl Engine {
    pub(super) fn size(&mut self, namespace: &str, settings: &PoolSettings) -> Desired {
        let now = self.clock.now();
        let floor = floor_now(settings, now, &self.config.timezone);
        let (window, bucket) = (self.config.rate_window, self.config.rate_bucket);
        let rate = self
            .claims
            .entry(namespace.to_owned())
            .or_insert_with(|| ClaimWindow::new(window, bucket))
            .peak(now);
        let desired = desired_size(rate, settings, floor);
        self.metrics.set_pool_desired(namespace, desired.clamped);
        self.metrics
            .set_claims_per_second(namespace, rate.per_second());
        desired
    }

    pub(super) async fn resize(
        &mut self,
        namespace: &str,
        owner: &Owner,
        desired: u32,
        current: u32,
    ) -> u32 {
        let now = self.clock.now();
        let rules = ScaleDownRules {
            cooldown: self.config.scale_down_cooldown,
            step: self.config.scale_down_step,
            interval: self.config.scale_down_step_interval,
        };
        let damped = damp(
            self.scale.get(namespace).copied().unwrap_or_default(),
            current,
            desired,
            now,
            &rules,
        );
        self.scale.insert(namespace.to_owned(), damped.state);
        if let Some(at) = damped.wake_at {
            self.wake_by(namespace, at);
        }
        if damped.replicas == current || !self.leadership.is_leader() {
            return current;
        }
        match self
            .writer
            .set_replicas(namespace, owner, damped.replicas)
            .await
        {
            Ok(_) => {
                tracing::info!(
                    pool = namespace,
                    replicas = damped.replicas,
                    "the replicas of a pool were set"
                );
                damped.replicas
            }
            Err(error) => {
                tracing::warn!(pool = namespace, %error, "the replicas will be written again");
                self.retry_soon(namespace);
                current
            }
        }
    }

    pub(super) async fn at_max_event(
        &mut self,
        namespace: &str,
        target: &EventTarget,
        settings: &PoolSettings,
        desired: &Desired,
    ) {
        if !desired.above_max(settings) {
            self.at_max.remove(namespace);
            return;
        }
        if !self.leadership.is_leader() || !self.at_max.insert(namespace.to_owned()) {
            return;
        }
        let message = format!(
            "the size {} that the claim rate asks for is above max_replicas {}",
            desired.raw, settings.max_replicas
        );
        if let Err(error) = self
            .api
            .create_event(namespace, target, PoolEvent::AtMax, &message)
            .await
        {
            self.action_failed(
                "an event could not be created",
                &replicaset_name(namespace),
                &error,
            );
            self.at_max.remove(namespace);
            self.retry_soon(namespace);
        }
    }
}
