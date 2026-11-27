use std::collections::HashSet;
use crate::cluster::DeleteOutcome;
use crate::facts::PodFacts;
use crate::rules::{Action, DeleteReason, ExpiryRules, judge_pod};

use super::{Engine, PodId, PoolId};

const CONFLICT_ATTEMPTS: usize = 4;

impl Engine {
    fn expiry_rules(&self) -> ExpiryRules {
        ExpiryRules {
            warm_max_age: self.config.warm_max_age,
            stuck_ttl: self.config.stuck_ttl,
            failed_pod_ttl: self.config.failed_pod_ttl,
            pending_alarm: self.config.pending_alarm,
            idle_ttl: self.config.idle_ttl,
            idle_keep_min_state_bytes: self.config.idle_keep_min_state_bytes,
        }
    }

    pub(crate) fn configured_pool(&self, facts: &PodFacts) -> Option<PoolId> {
        let id = facts.pool_id.as_ref()?;
        self.pools
            .iter()
            .find(|pool| pool.namespace == facts.namespace && &pool.id == id)
            .map(|_| (facts.namespace.clone(), id.clone()))
    }

    pub(crate) async fn judge(&mut self, id: &PodId) {
        let mut conflicts = 0;
        loop {
            let Some(tracked) = self.pods.get(id) else {
                return;
            };
            let facts = tracked.facts.clone();
            let failed_since = tracked.failed_since;
            let rules = self.expiry_rules();
            let now = self.clock.now();
            let judgment = judge_pod(&facts, &rules, failed_since, false, now);
            if let Some(tracked) = self.pods.get_mut(id) {
                tracked.recheck_at = judgment.recheck_at;
            }
            match judgment.action {
                Action::Keep => {
                    self.unreadable.remove(&facts.uid);
                    if let Some(due) = self
                        .configured_pool(&facts)
                        .and_then(|pool| self.pending_due.get_mut(&pool))
                    {
                        due.remove(&facts.uid);
                    }
                    return;
                }
                Action::Unreadable(field) => {
                    tracing::warn!(
                        pod = facts.name,
                        field,
                        "a pod annotation is missing or unreadable"
                    );
                    self.unreadable.insert(facts.uid.clone());
                    return;
                }
                Action::PendingAlarm => {
                    self.unreadable.remove(&facts.uid);
                    if let Some(pool) = self.configured_pool(&facts) {
                        self.pending_due
                            .entry(pool)
                            .or_default()
                            .insert(facts.uid.clone(), facts.name.clone());
                    }
                    return;
                }
                Action::Delete(reason) => {
                    self.unreadable.remove(&facts.uid);
                    if !self.leadership.is_leader() {
                        if let Some(tracked) = self.pods.get_mut(id) {
                            tracked.recheck_at = None;
                        }
                        return;
                    }
                    if !self.delete(id, reason, &mut conflicts).await {
                        return;
                    }
                }
            }
        }
    }

    async fn delete(&mut self, id: &PodId, reason: DeleteReason, conflicts: &mut usize) -> bool {
        let Some(tracked) = self.pods.get(id) else {
            return false;
        };
        if !self.leadership.is_leader() {
            return false;
        }
        let version = tracked.facts.version();
        match self.api.delete_pod(&id.0, &id.1, &version).await {
            Ok(DeleteOutcome::Deleted) => {
                tracing::info!(pod = id.1, reason = reason.label(), "a pod was deleted");
                self.metrics.pod_deleted(reason.label());
                let mut touched: HashSet<PoolId> = HashSet::new();
                self.remove(id, &mut touched);
                false
            }
            Ok(DeleteOutcome::Gone) => {
                let mut touched: HashSet<PoolId> = HashSet::new();
                self.remove(id, &mut touched);
                false
            }
            Ok(DeleteOutcome::Conflict) => {
                *conflicts += 1;
                self.reread(id, *conflicts).await
            }
            Err(error) => {
                self.action_failed("a pod could not be deleted", &id.1, &error);
                let retry_at = self.clock.now().plus(self.retry_period());
                if let Some(tracked) = self.pods.get_mut(id) {
                    tracked.recheck_at = Some(retry_at);
                }
                false
            }
        }
    }

    async fn reread(&mut self, id: &PodId, conflicts: usize) -> bool {
        match self.api.get_pod(&id.0, &id.1).await {
            Ok(None) => {
                let mut touched: HashSet<PoolId> = HashSet::new();
                self.remove(id, &mut touched);
                false
            }
            Ok(Some(fresh)) => {
                let mut touched: HashSet<PoolId> = HashSet::new();
                self.apply(fresh, true, &mut touched);
                for pool in touched {
                    self.evaluate_pool_id(&pool).await;
                }
                if conflicts < CONFLICT_ATTEMPTS {
                    return true;
                }
                let retry_at = self.clock.now().plus(self.retry_period());
                if let Some(tracked) = self.pods.get_mut(id) {
                    tracked.recheck_at = Some(retry_at);
                }
                false
            }
            Err(error) => {
                self.action_failed("a pod could not be read again", &id.1, &error);
                let retry_at = self.clock.now().plus(self.retry_period());
                if let Some(tracked) = self.pods.get_mut(id) {
                    tracked.recheck_at = Some(retry_at);
                }
                false
            }
        }
    }
}
