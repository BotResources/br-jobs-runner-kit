#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    #[error("the document is not a valid keeper configuration: {0}")]
    Document(String),
    #[error("the key {key} is required")]
    Missing { key: String },
    #[error("the key {key} must be above zero")]
    NotAboveZero { key: String },
    #[error("the key {key} must not be negative")]
    Negative { key: String },
    #[error("the key {key} is not valid: {reason}")]
    Invalid { key: String, reason: String },
    #[error("the pool {pool} has min_replicas above max_replicas")]
    MinAboveMax { pool: String },
    #[error("rate_bucket_seconds is above rate_window_seconds")]
    BucketAboveWindow,
    #[error("scale_down_step must be at least 1")]
    StepBelowOne,
    #[error(
        "lease_duration_seconds must be above renew_deadline_seconds, which must be above retry_period_seconds"
    )]
    LeaseOrder,
    #[error("the time zone {0:?} is not a known name")]
    UnknownTimezone(String),
    #[error("the window {index} of the floors of the pool {pool} ends no later than it begins")]
    WindowEndsNoLater { pool: String, index: usize },
    #[error("two pools have the id {0:?}")]
    DuplicatePoolId(String),
    #[error("two pools have the replica set {replicaset:?} in the namespace {namespace:?}")]
    DuplicateReplicaSet {
        namespace: String,
        replicaset: String,
    },
    #[error("the keeper has no pool")]
    NoPools,
}
