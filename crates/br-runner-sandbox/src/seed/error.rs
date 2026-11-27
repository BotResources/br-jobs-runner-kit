#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SeedError {
    #[error("the seed entry path {path:?} is not relative, is empty or holds a parent component")]
    Path { path: String },
    #[error("the seed archive could not be built: {0}")]
    Archive(String),
}

impl SeedError {
    pub fn is_refusal(&self) -> bool {
        matches!(self, Self::Path { .. })
    }
}
