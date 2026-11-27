use crate::cluster::{FenceObservation, Read};
use crate::pool::{FenceVerdict, PoolRef, is_duplicate};

use super::Engine;

impl Engine {
    pub(crate) async fn reconcile_all(&mut self) {
        let Some(managed) = self.managed() else {
            return;
        };
        for namespace in managed {
            self.reconcile(&namespace).await;
        }
    }

    pub(crate) async fn reconcile(&mut self, namespace: &str) {
        self.pool_wake.remove(namespace);
        let pools = match self.views.pools(namespace) {
            Read::Observed(found) => found,
            Read::NotObserved(reason) => {
                tracing::debug!(namespace, reason, "the resources are not observed");
                return;
            }
        };
        if pools.is_empty() {
            self.forget_pool(namespace);
            return;
        }
        self.refresh_warm_ready(namespace);
        if !self.leadership.is_leader() {
            return;
        }
        let pods_synced = self.pods_synced(namespace);
        if !self.seen.contains(namespace) {
            if !self.first_sight(namespace) {
                return;
            }
            self.seen.insert(namespace.to_owned());
        }
        let verdict = if pods_synced {
            self.fence_verdict(namespace).await
        } else {
            None
        };
        let refs: Vec<PoolRef> = pools.iter().map(PoolRef::of).collect();
        for pool in &pools {
            let duplicate = is_duplicate(&PoolRef::of(pool), &refs);
            self.reconcile_pool(namespace, pool, verdict.as_ref(), duplicate, pods_synced)
                .await;
        }
    }

    fn first_sight(&self, namespace: &str) -> bool {
        self.pods_synced(namespace)
            && self.views.namespace(namespace).observed().is_some()
            && self.views.binding().observed().is_some()
    }

    async fn fence_verdict(&mut self, namespace: &str) -> Option<FenceVerdict> {
        match self.observer.fence(namespace).await {
            FenceObservation::Observed(inputs) => {
                Some(inputs.verdict(&self.config.managed_namespace_selector))
            }
            FenceObservation::NotManaged => None,
            FenceObservation::NotObserved(reason) => {
                self.action_failed(
                    "the fence of a namespace is not observed",
                    namespace,
                    &reason,
                );
                self.retry_soon(namespace);
                None
            }
        }
    }
}
