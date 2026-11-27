use serde_json::Value;

use super::*;

impl Bed {
    pub fn replicas(&self) -> Option<i32> {
        self.kube
            .raw_replica_set(NS, RS)
            .and_then(|set| set.spec)
            .and_then(|spec| spec.replicas)
    }

    pub fn replica_writes(&self) -> Vec<i32> {
        self.kube
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Replicas { replicas, .. } => Some(replicas),
                _ => None,
            })
            .collect()
    }

    pub fn deletes(&self) -> Vec<String> {
        self.kube
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Delete { name, .. } => Some(name),
                _ => None,
            })
            .collect()
    }

    pub fn pool_writes(&self) -> Vec<Call> {
        self.kube
            .calls()
            .into_iter()
            .filter(|call| {
                matches!(
                    call,
                    Call::CreateReplicaSet { .. }
                        | Call::ApplyTemplate { .. }
                        | Call::DeleteReplicaSet { .. }
                        | Call::Replicas { .. }
                        | Call::Condition { .. }
                        | Call::Delete { .. }
                        | Call::Event { .. }
                )
            })
            .collect()
    }

    pub fn creations(&self) -> Vec<Value> {
        self.kube
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::CreateReplicaSet { body, .. } => Some(body),
                _ => None,
            })
            .collect()
    }

    pub fn template_writes(&self) -> usize {
        self.kube
            .calls()
            .iter()
            .filter(|call| matches!(call, Call::ApplyTemplate { .. }))
            .count()
    }

    pub fn replicaset_deletes(&self) -> usize {
        self.kube
            .calls()
            .iter()
            .filter(|call| matches!(call, Call::DeleteReplicaSet { .. }))
            .count()
    }

    pub fn status_writes(&self) -> usize {
        self.kube
            .calls()
            .iter()
            .filter(|call| matches!(call, Call::PoolStatus { .. }))
            .count()
    }

    pub fn status_of(&self, name: &str) -> Option<Value> {
        self.kube.pool_status(NS, name)
    }

    pub fn status(&self) -> Option<Value> {
        self.kube.pool_status(NS, "main")
    }

    pub fn condition(&self, kind: &str) -> Option<(String, String)> {
        let status = self.status()?;
        status["conditions"]
            .as_array()?
            .iter()
            .find(|condition| condition["type"] == kind)
            .map(|condition| {
                (
                    condition["status"].as_str().unwrap_or_default().to_owned(),
                    condition["reason"].as_str().unwrap_or_default().to_owned(),
                )
            })
    }

    pub fn message(&self, kind: &str) -> Option<String> {
        self.status()?["conditions"]
            .as_array()?
            .iter()
            .find(|condition| condition["type"] == kind)
            .and_then(|condition| condition["message"].as_str().map(str::to_owned))
    }

    pub fn bucket(&self) -> u64 {
        self.engine.config.rate_bucket.as_secs()
    }

    pub fn metrics_text(&self) -> String {
        self.metrics.render(self.leadership.is_leader())
    }

    pub fn errors(&self) -> u64 {
        self.metrics_text()
            .lines()
            .find_map(|line| line.strip_prefix("br_sandbox_keeper_sweep_errors_total "))
            .and_then(|count| count.parse().ok())
            .unwrap_or(0)
    }

    pub fn has_metric(&self, line: &str) -> bool {
        self.metrics_text().lines().any(|text| text == line)
    }

    pub fn set_pool_conditions(&self, conditions: Vec<crate::facts::ConditionFacts>) {
        self.kube.set_replica_set_conditions(NS, RS, conditions);
    }

    pub fn rs_conditions(&self) -> Vec<crate::facts::ConditionFacts> {
        self.kube
            .raw_replica_set(NS, RS)
            .map(|set| replica_set_facts(&set).conditions)
            .unwrap_or_default()
    }

    pub fn template_hash(&self) -> Option<String> {
        self.kube
            .raw_replica_set(NS, RS)?
            .spec?
            .template?
            .metadata?
            .annotations?
            .get("br.sandbox/template-hash")
            .cloned()
    }

    pub fn add_managed(&self, namespace: &str) {
        self.kube.put_namespace(managed(namespace));
    }
}
