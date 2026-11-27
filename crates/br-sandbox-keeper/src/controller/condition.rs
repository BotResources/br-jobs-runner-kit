use k8s_openapi::api::apps::v1::ReplicaSet;

use crate::cluster::{EventTarget, PoolEvent};
use crate::convert::replica_set_facts;
use crate::pool::{Owner, replicaset_name};
use crate::rules::{PoolJudgement, condition_write};

use super::Engine;

const REPLICA_FAILURE: &str = "ReplicaFailure";

pub(super) fn replica_failure(replica_set: &ReplicaSet) -> Option<String> {
    replica_set
        .status
        .as_ref()?
        .conditions
        .iter()
        .flatten()
        .find(|condition| condition.type_ == REPLICA_FAILURE && condition.status == "True")
        .map(|condition| {
            condition
                .message
                .clone()
                .or_else(|| condition.reason.clone())
                .unwrap_or_else(|| "the ReplicaSet reports a replica failure".to_owned())
        })
}

pub(super) fn target_of(namespace: &str, replica_set: &ReplicaSet) -> EventTarget {
    EventTarget::new(
        replicaset_name(namespace),
        replica_set.metadata.uid.clone().unwrap_or_default(),
    )
}

impl Engine {
    pub(super) async fn mirror_condition(
        &mut self,
        namespace: &str,
        owner: &Owner,
        replica_set: &ReplicaSet,
        judged: &PoolJudgement,
    ) {
        let held = replica_set_facts(replica_set);
        let Some(write) = condition_write(&held.conditions, judged, self.clock.now()) else {
            return;
        };
        if !self.leadership.is_leader() {
            return;
        }
        if let Err(error) = self.writer.set_condition(namespace, owner, &write).await {
            tracing::warn!(pool = namespace, %error, "the condition will be written again");
            self.retry_soon(namespace);
        }
    }

    pub(super) async fn pending_events(&mut self, namespace: &str, target: &EventTarget) {
        let due: Vec<(String, String)> = self
            .pending_due
            .get(namespace)
            .map(|due| {
                due.iter()
                    .map(|(uid, name)| (uid.clone(), name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        for (uid, name) in due {
            if self.pending_notified.contains(&uid) || !self.leadership.is_leader() {
                continue;
            }
            let message = format!("the warm pod {name} is Pending and no container is failing");
            match self
                .api
                .create_event(namespace, target, PoolEvent::WarmPodsPending, &message)
                .await
            {
                Ok(()) => {
                    self.pending_notified.insert(uid);
                }
                Err(error) => {
                    self.action_failed("an event could not be created", &name, &error);
                    self.retry_soon(namespace);
                }
            }
        }
    }
}
