use std::collections::HashSet;

use futures::StreamExt;
use futures::stream::BoxStream;
use tokio_util::sync::CancellationToken;

use crate::cluster::PodEvent;

use super::{Engine, NamespacedEvents, PodId};

impl Engine {
    fn watch(
        &self,
        namespace: &str,
        stop: &CancellationToken,
    ) -> BoxStream<'static, (String, PodEvent)> {
        let owned = namespace.to_owned();
        self.api
            .watch_pods(namespace)
            .map(move |event| (owned.clone(), event))
            .take_until(stop.clone().cancelled_owned())
            .boxed()
    }

    pub(super) fn follow(&mut self, namespaces: &[String], events: &mut NamespacedEvents) {
        for namespace in namespaces {
            if !self.watched.contains_key(namespace) {
                let stop = CancellationToken::new();
                events.push(self.watch(namespace, &stop));
                self.watched.insert(namespace.clone(), stop);
            }
        }
        let found: HashSet<&String> = namespaces.iter().collect();
        let lost: Vec<String> = self
            .watched
            .keys()
            .filter(|namespace| !found.contains(namespace))
            .cloned()
            .collect();
        for namespace in lost {
            self.release(&namespace);
        }
    }

    fn release(&mut self, namespace: &str) {
        if let Some(stop) = self.watched.remove(namespace) {
            stop.cancel();
        }
        self.synced.remove(namespace);
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
        let pools: Vec<_> = self
            .config
            .pools
            .iter()
            .filter(|pool| pool.namespace == namespace)
            .cloned()
            .collect();
        for pool in &pools {
            let id = super::pool_id(pool);
            self.claims.remove(&id);
            self.scale.remove(&id);
            self.pool_wake.remove(&id);
            self.at_max.remove(&id);
            self.pending_due.remove(&id);
            self.refresh_pool_metrics(pool);
        }
        self.refresh_fleet_metrics();
        tracing::info!(
            namespace,
            "a namespace is no longer managed and is no longer served"
        );
    }
}
