#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("the key {0:?} is not a valid object key")]
    Key(String),
    #[error("the store does not honour a conditional create")]
    ConditionalCreateUnsupported,
    #[error("the store call failed: {0}")]
    Call(String),
}
