use br_llm_clients_core::BuildError;
use br_runner_jobs::Removal;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RunnerToolError {
    #[error(transparent)]
    Spec(BuildError),
    #[error(transparent)]
    RunnerType(br_runner_jobs::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the create of the child Job was removed from the delivery queue: {removal:?}")]
pub struct CreateRemoved {
    pub removal: Removal,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the end of the sync call is of a kind this library does not know")]
pub struct UnknownEnd;
