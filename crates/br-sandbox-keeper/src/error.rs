use crate::config::ConfigError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeeperError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("the Kubernetes API is not reachable at start: {0}")]
    ApiUnreachable(String),
    #[error("the object store is not reachable at start: {0}")]
    StoreUnreachable(String),
    #[error("the bucket {0:?} is absent at start")]
    BucketAbsent(String),
    #[error("the environment variable {name} is unusable: {reason}")]
    Environment { name: &'static str, reason: String },
    #[error("the health address {address} cannot be bound: {reason}")]
    Listen { address: String, reason: String },
    #[error("the task {0} ended before the stop signal")]
    TaskEnded(&'static str),
    #[error(
        "the namespace {namespace:?} of the pool {pool:?} does not match the managed namespace selector"
    )]
    PoolNamespaceNotManaged { pool: String, namespace: String },
}
