use k8s_openapi::api::apps::v1::ReplicaSet;

use crate::cluster::ReplicaSetRead;
use crate::facts::{Mark, PodFacts, PoolState};
use crate::pool::{Counts, Observation, ReplicaSetView};
use crate::rules::judge_pool;

use super::Engine;
use super::condition::replica_failure;

impl Engine {
    pub(super) fn observation(
        &self,
        namespace: &str,
        pods_synced: bool,
        held: Option<&ReplicaSetRead>,
        desired: Option<u32>,
        hash: Option<&str>,
    ) -> Option<Observation> {
        let held = held.filter(|_| pods_synced)?;
        let warm: Vec<&PodFacts> = self
            .pool_pods(namespace, PoolState::Warm)
            .filter(|pod| !pod.deleting)
            .collect();
        let (view, replicas, held_hash) = match held {
            ReplicaSetRead::Absent => (ReplicaSetView::Absent, None, None),
            ReplicaSetRead::NotOwned(_) => (ReplicaSetView::NotOwned, None, None),
            ReplicaSetRead::Owned(replica_set) => (
                ReplicaSetView::Owned {
                    refusal: replica_failure(replica_set),
                },
                Some(replicas_of(replica_set)),
                self.held_hash(replica_set),
            ),
        };
        let hash = hash.map(str::to_owned).or(held_hash);
        let count = |total: usize| u32::try_from(total).unwrap_or(u32::MAX);
        let ready = warm.iter().filter(|pod| pod.ready).count();
        let updated = hash.as_deref().map_or(0, |hash| {
            warm.iter()
                .filter(|pod| pod.template_hash == Mark::Value(hash.to_owned()))
                .count()
        });
        let judged = judge_pool(warm.iter().copied());
        Some(Observation::new(
            Counts::new(
                desired.or(replicas).unwrap_or(0),
                count(warm.len()),
                count(ready),
                count(updated),
            ),
            view,
            judged,
            self.clock.now(),
        ))
    }
}

pub(super) fn replicas_of(replica_set: &ReplicaSet) -> u32 {
    let replicas = replica_set
        .spec
        .as_ref()
        .and_then(|spec| spec.replicas)
        .unwrap_or(0);
    u32::try_from(replicas).unwrap_or(0)
}
