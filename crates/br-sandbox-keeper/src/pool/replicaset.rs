use br_runner_sandbox::{POOL_WARM, PodKeys};
use serde_json::{Value, json};

use super::names::{pool_id, replicaset_name};
use super::ownership::{Owner, owner_reference};
use super::template::RenderedTemplate;

pub fn replica_set_body(
    namespace: &str,
    owner: &Owner,
    replicas: i32,
    rendered: &RenderedTemplate,
    keys: &PodKeys,
) -> Value {
    json!({
        "apiVersion": "apps/v1",
        "kind": "ReplicaSet",
        "metadata": {
            "name": replicaset_name(namespace),
            "namespace": namespace,
            "ownerReferences": [owner_reference(owner)],
        },
        "spec": {
            "replicas": replicas,
            "selector": {"matchLabels": {
                keys.pool(): POOL_WARM,
                keys.pool_id(): pool_id(namespace),
            }},
            "template": rendered.template,
        },
    })
}

#[cfg(test)]
mod tests;
