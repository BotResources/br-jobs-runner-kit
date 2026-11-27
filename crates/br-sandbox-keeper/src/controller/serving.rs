use std::collections::{BTreeSet, HashSet};

use crate::facts::{PodFacts, PoolState};

use super::{Engine, PodId};

impl Engine {
    pub(super) fn managed(&self) -> Option<Vec<String>> {
        self.views.managed_namespaces().observed().map(|found| {
            let names: BTreeSet<String> = found
                .into_iter()
                .filter_map(|namespace| namespace.metadata.name)
                .collect();
            names.into_iter().collect()
        })
    }

    pub(super) fn pods_synced(&self, namespace: &str) -> bool {
        self.synced.contains(namespace) && self.views.pods(namespace).observed().is_some()
    }

    pub(super) fn pool_pods<'a>(
        &'a self,
        namespace: &'a str,
        state: PoolState,
    ) -> impl Iterator<Item = &'a PodFacts> {
        self.pods
            .values()
            .map(|tracked| &tracked.facts)
            .filter(move |pod| {
                pod.namespace == namespace
                    && pod.pool_id.as_deref() == Some(namespace)
                    && pod.pool_state == state
            })
    }

    pub(super) fn refresh_warm_ready(&self, namespace: &str) {
        let ready = self
            .pool_pods(namespace, PoolState::Warm)
            .filter(|pod| pod.ready)
            .count();
        self.metrics
            .set_pool_warm_ready(namespace, u32::try_from(ready).unwrap_or(u32::MAX));
    }

    pub(super) fn forget_pool(&mut self, namespace: &str) {
        self.claims.remove(namespace);
        self.scale.remove(namespace);
        self.pool_wake.remove(namespace);
        self.at_max.remove(namespace);
        self.pending_due.remove(namespace);
        self.unhashed.remove(namespace);
        self.status_written.retain(|(ns, _), _| ns != namespace);
        self.metrics.forget_pool(namespace);
    }

    pub(super) fn release_unmanaged(&mut self) {
        let Some(managed) = self.managed() else {
            return;
        };
        let kept: HashSet<&String> = managed.iter().collect();
        let known: BTreeSet<String> = self
            .synced
            .iter()
            .cloned()
            .chain(self.pods.keys().map(|(namespace, _)| namespace.clone()))
            .chain(self.claims.keys().cloned())
            .chain(self.scale.keys().cloned())
            .collect();
        for namespace in known.iter().filter(|namespace| !kept.contains(namespace)) {
            self.release(namespace);
        }
    }

    fn release(&mut self, namespace: &str) {
        self.synced.remove(namespace);
        self.seen.remove(namespace);
        let ids: Vec<PodId> = self
            .pods
            .keys()
            .filter(|(pod_namespace, _)| pod_namespace == namespace)
            .cloned()
            .collect();
        let mut touched = HashSet::new();
        for id in ids {
            self.remove(&id, &mut touched);
        }
        self.forget_pool(namespace);
        self.refresh_fleet_metrics();
        tracing::info!(
            namespace,
            "a namespace is no longer managed and is no longer served"
        );
    }
}
