mod definition;
mod object;
mod schema;
mod spec;
mod status;
#[cfg(test)]
mod tests;

pub use definition::{DefinitionError, definition, definition_yaml};
pub use object::SandboxPool;
pub use spec::{FloorSpec, PoolSize, SandboxPoolSpec, SizeBounds};
pub use status::SandboxPoolStatus;

pub const GROUP: &str = "sandbox.botresources.ai";
pub const VERSION: &str = "v1alpha1";
pub const KIND: &str = "SandboxPool";
pub const LIST_KIND: &str = "SandboxPoolList";
pub const PLURAL: &str = "sandboxpools";
pub const SINGULAR: &str = "sandboxpool";
pub const MAX_FLOORS: usize = 16;
pub const MAX_TIME_LENGTH: usize = 64;
pub const DEFAULT_REFILL_SECONDS: u32 = 3;
pub const DEFAULT_MARGIN: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
#[non_exhaustive]
pub struct SpecError(String);

impl SpecError {
    fn new(reason: String) -> Self {
        Self(reason)
    }
}
