#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClusterError {
    #[error("the pod {0} was changed by another writer")]
    Conflict(String),
    #[error("the pod {0} is gone")]
    Gone(String),
    #[error("the cluster configuration is not usable: {0}")]
    Config(String),
    #[error("the cluster call failed: {0}")]
    Call(String),
}
