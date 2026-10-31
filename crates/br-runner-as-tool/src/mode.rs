use uuid::Uuid;

use br_runner_jobs::CallMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolMode {
    Sync,
    Background { caller_address: Uuid },
}

impl ToolMode {
    pub(crate) fn call_mode(self) -> CallMode {
        match self {
            Self::Sync => CallMode::Sync,
            Self::Background { caller_address } => CallMode::Background { caller_address },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PayloadKind {
    #[default]
    Inline,
    Kept,
}
