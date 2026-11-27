use std::collections::HashSet;

use crate::cluster::DeleteOutcome;
use crate::rules::{Action, DeleteReason, ExpiryRules, judge_pod};

use super::events::pool_of;
use super::{Engine, PodId};

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

    pub(crate) async fn judge(&mut self, id: &PodId) {
        self.judge_with(id, false).await;
    }

    pub(crate) async fn judge_selected(&mut self, id: &PodId) {
        self.judge_with(id, true).await;
    }

    async fn judge_with(&mut self, id: &PodId, selected: bool) {
        let mut conflicts = 0;
        loop {
            let Some(tracked) = self.pods.get(id) else {
                return;
            };
            let facts = tracked.facts.clone();
            let failed_since = tracked.failed_since;
            let rules = self.expiry_rules();
            let now = self.clock.now();
            let judgment = judge_pod(&facts, &rules, failed_since, selected, now);
            if !selected && let Some(tracked) = self.pods.get_mut(id) {
                tracked.recheck_at = judgment.recheck_at;
            }
            match judgment.action {
                Action::Keep => {
                    self.unreadable.remove(&facts.uid);
                    if let Some(due) =
                        pool_of(&facts).and_then(|pool| self.pending_due.get_mut(&pool))
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
                    if let Some(pool) = pool_of(&facts) {
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
                        if !selected && let Some(tracked) = self.pods.get_mut(id) {
                            tracked.recheck_at = None;
                        }
                        return;
                    }
                    if !self.delete(id, reason, selected, &mut conflicts).await {
                        return;
                    }
                }
            }
        }
    }

    async fn delete(
        &mut self,
        id: &PodId,
        reason: DeleteReason,
        selected: bool,
        conflicts: &mut usize,
    ) -> bool {
        let Some(tracked) = self.pods.get(id) else {
            return false;
        };
        if !self.leadership.is_leader() {
            return false;
        }
        let version = tracked.facts.version();
        match self.writer.delete_pod(&id.0, &id.1, &version).await {
            Ok(DeleteOutcome::Deleted) => {
                tracing::info!(pod = id.1, reason = reason.label(), "a pod was deleted");
                self.metrics.pod_deleted(reason.label());
                self.forget_pod(id);
                false
            }
            Ok(DeleteOutcome::Gone) => {
                self.forget_pod(id);
                false
            }
            Ok(DeleteOutcome::Conflict) => {
                *conflicts += 1;
                self.reread(id, *conflicts).await
            }
            Err(error) => {
                tracing::warn!(pod = id.1, %error, "a pod delete will be tried again");
                self.recheck_later(id, selected);
                false
            }
        }
    }

    fn forget_pod(&mut self, id: &PodId) {
        let mut touched: HashSet<String> = HashSet::new();
        self.remove(id, &mut touched);
        self.wake_now(touched);
    }

    fn wake_now(&mut self, namespaces: HashSet<String>) {
        let now = self.clock.now();
        for namespace in namespaces {
            self.wake_by(&namespace, now);
        }
    }

    fn recheck_later(&mut self, id: &PodId, selected: bool) {
        let retry_at = self.clock.now().plus(self.retry_period());
        if !selected && let Some(tracked) = self.pods.get_mut(id) {
            tracked.recheck_at = Some(retry_at);
        }
        self.wake_by(&id.0, retry_at);
    }

    async fn reread(&mut self, id: &PodId, conflicts: usize) -> bool {
        match self.api.get_pod(&id.0, &id.1).await {
            Ok(None) => {
                self.forget_pod(id);
                false
            }
            Ok(Some(fresh)) => {
                let mut touched: HashSet<String> = HashSet::new();
                self.apply(fresh, true, &mut touched);
                self.wake_now(touched);
                if conflicts < CONFLICT_ATTEMPTS {
                    return true;
                }
                self.recheck_later(id, false);
                false
            }
            Err(error) => {
                self.action_failed("a pod could not be read again", &id.1, &error);
                self.recheck_later(id, false);
                false
            }
        }
    }
}
