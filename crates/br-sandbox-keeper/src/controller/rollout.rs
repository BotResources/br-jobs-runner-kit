use std::collections::HashSet;

use crate::facts::{PodFacts, PoolState};
use crate::rules::{RolloutPlan, RolloutState, plan_rollout};

use super::Engine;

impl Engine {
    pub(super) async fn roll(&mut self, namespace: &str, hash: &str, replicas: u32) {
        let plan = {
            let warm: Vec<&PodFacts> = self.pool_pods(namespace, PoolState::Warm).collect();
            plan_rollout(warm, hash, self.config.scale_down_step, replicas)
        };
        self.note_unhashed(namespace, &plan);
        match plan.state {
            RolloutState::Step => tracing::info!(
                pool = namespace,
                stale = plan.stale,
                step = plan.delete.len(),
                "a rollout step removes stale warm pods"
            ),
            RolloutState::Stopped => tracing::warn!(
                pool = namespace,
                stale = plan.stale,
                "the rollout is stopped while a warm pod of the current template fails"
            ),
            RolloutState::Waiting(wait) => tracing::debug!(
                pool = namespace,
                ?wait,
                "the rollout waits before the next step"
            ),
            RolloutState::Settled => {}
        }
        for name in plan.delete {
            self.judge_selected(&(namespace.to_owned(), name)).await;
        }
    }

    fn note_unhashed(&mut self, namespace: &str, plan: &RolloutPlan) {
        let found: HashSet<String> = plan
            .unreadable
            .iter()
            .filter_map(|name| self.pods.get(&(namespace.to_owned(), name.clone())))
            .map(|tracked| tracked.facts.uid.clone())
            .collect();
        let before = self.unhashed.insert(namespace.to_owned(), found.clone());
        for name in &plan.unreadable {
            let uid = self
                .pods
                .get(&(namespace.to_owned(), name.clone()))
                .map(|tracked| &tracked.facts.uid);
            if uid.is_some_and(|uid| before.as_ref().is_none_or(|held| !held.contains(uid))) {
                tracing::warn!(
                    pod = name,
                    "a warm pod has no readable template hash and is never judged stale"
                );
            }
        }
    }
}
