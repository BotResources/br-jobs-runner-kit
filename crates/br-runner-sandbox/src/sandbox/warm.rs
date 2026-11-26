use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures::StreamExt;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::cluster::{Cluster, LabelSelector, PodEvent, PodView};

pub(super) struct WarmPods {
    selector: LabelSelector,
    pods: Mutex<BTreeMap<String, PodView>>,
    synced: watch::Sender<bool>,
}

impl WarmPods {
    pub fn new(selector: LabelSelector) -> Self {
        Self {
            selector,
            pods: Mutex::new(BTreeMap::new()),
            synced: watch::channel(false).0,
        }
    }

    pub fn spawn(self: &Arc<Self>, cluster: Arc<dyn Cluster>) -> JoinHandle<()> {
        let warm = self.clone();
        tokio::spawn(async move {
            let mut events = cluster.watch_pods(&warm.selector);
            while let Some(event) = events.next().await {
                warm.apply(event);
            }
        })
    }

    fn apply(&self, event: PodEvent) {
        let mut pods = self.pods.lock().unwrap_or_else(PoisonError::into_inner);
        match event {
            PodEvent::Snapshot(views) => {
                pods.clear();
                for view in views {
                    if self.selector.matches(&view.labels) {
                        pods.insert(view.name.clone(), view);
                    }
                }
                self.synced.send_replace(true);
            }
            PodEvent::Applied(view) => {
                if self.selector.matches(&view.labels) {
                    pods.insert(view.name.clone(), view);
                } else {
                    pods.remove(&view.name);
                }
            }
            PodEvent::Deleted(name) => {
                pods.remove(&name);
            }
        }
    }

    pub async fn wait_synced(&self, limit: Duration) -> bool {
        let mut synced = self.synced.subscribe();
        tokio::time::timeout(limit, synced.wait_for(|synced| *synced))
            .await
            .is_ok_and(|result| result.is_ok())
    }

    pub fn pick(&self, skipped: &HashSet<String>) -> Option<PodView> {
        self.pods
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .find(|pod| pod.is_ready() && !skipped.contains(&pod.name))
            .cloned()
    }

    pub fn forget(&self, name: &str) {
        self.pods
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name);
    }
}
