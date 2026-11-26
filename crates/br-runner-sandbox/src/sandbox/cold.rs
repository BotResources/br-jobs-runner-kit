use br_agentic_harness_contract::TurnRef;
use futures::StreamExt;

use crate::cluster::{LabelSelector, PodEvent, ReplicaSetView};
use crate::decide::{cold_pod_manifest, cold_pod_marks};
use crate::error::Error;
use crate::keys::POOL_CLAIMED;

use super::guard::OpenGuard;
use super::state::Sandbox;

fn settled(event: PodEvent, name: &str) -> Option<bool> {
    match event {
        PodEvent::Snapshot(views) => views
            .iter()
            .find(|view| view.name == name)
            .map_or(Some(false), |view| view.is_ready().then_some(true)),
        PodEvent::Applied(view) if view.name == name => view.is_ready().then_some(true),
        PodEvent::Deleted(deleted) if deleted == name => Some(false),
        PodEvent::Applied(_) | PodEvent::Deleted(_) => None,
    }
}

impl Sandbox {
    pub(super) async fn cold_pod(
        &self,
        turn: &TurnRef,
        pool: &ReplicaSetView,
        guard: &mut OpenGuard,
    ) -> Result<String, Error> {
        let env = self.env();
        let marks = cold_pod_marks(
            &env.pod_keys(),
            env.config.pool_id(),
            turn.thread,
            turn.agent,
            env.clock.now(),
        );
        let manifest = cold_pod_manifest(&pool.template, &format!("{}-cold-", pool.name), &marks)?;
        let created = env.cluster.create_pod(&manifest).await?;
        guard.hold(created.name.clone());
        if created.is_ready() || self.wait_ready(&created.name, turn).await {
            return Ok(created.name);
        }
        self.discard(guard).await;
        Err(self.image_error(env.config.open_timeout()).await)
    }

    async fn wait_ready(&self, name: &str, turn: &TurnRef) -> bool {
        let env = self.env();
        let keys = env.pod_keys();
        let selector = LabelSelector::new()
            .with(keys.pool_id(), env.config.pool_id().as_str())
            .with(keys.pool(), POOL_CLAIMED)
            .with(keys.thread(), turn.thread.to_string())
            .with(keys.agent(), turn.agent.to_string());
        let mut events = env.cluster.watch_pods(&selector);
        let waiting = async {
            while let Some(event) = events.next().await {
                if let Some(ready) = settled(event, name) {
                    return ready;
                }
            }
            false
        };
        tokio::time::timeout(env.config.open_timeout(), waiting)
            .await
            .unwrap_or(false)
    }
}
