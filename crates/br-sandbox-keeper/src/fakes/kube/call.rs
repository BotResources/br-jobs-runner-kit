use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    CreateReplicaSet {
        name: String,
        body: Value,
    },
    ApplyTemplate {
        name: String,
        template: Value,
    },
    DeleteReplicaSet {
        name: String,
        uid: String,
    },
    PoolStatus {
        name: String,
        status: Value,
    },
    Delete {
        name: String,
        uid: String,
        resource_version: String,
    },
    Replicas {
        name: String,
        replicas: i32,
    },
    Condition {
        name: String,
        status: String,
        reason: String,
        message: String,
    },
    Event {
        reason: &'static str,
        message: String,
    },
    LeaseCreate {
        holder: String,
    },
    LeaseUpdate {
        holder: String,
        resource_version: String,
    },
}
