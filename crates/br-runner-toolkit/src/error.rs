use br_agentic_contract::{InboxError, PortFault, ThreadError};
use br_graph_contract::Reason;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PortErrorCode {
    NotStarted,
    UnknownAgent,
    UnknownInstance,
    NoLiveInstance,
    NoAuthorName,
    NoInbox,
    JobsRefused,
    JobsFailed,
    ThreadsRefused,
    ThreadsFailed,
    AdditionRefused,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code:?}: {reason}")]
pub struct PortError {
    code: PortErrorCode,
    reason: String,
}

impl PortError {
    pub fn new(code: PortErrorCode, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
        }
    }

    pub fn code(&self) -> PortErrorCode {
        self.code
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl From<PortError> for PortFault {
    fn from(error: PortError) -> Self {
        PortFault::new(error)
    }
}

impl From<PortError> for ThreadError {
    fn from(error: PortError) -> Self {
        match error.code {
            PortErrorCode::AdditionRefused => ThreadError::Refused(Reason::new(error.reason)),
            _ => ThreadError::Fault(error.into()),
        }
    }
}

impl From<PortError> for InboxError {
    fn from(error: PortError) -> Self {
        InboxError::Fault(error.into())
    }
}

pub(crate) fn from_threads(error: br_runner_threads::Error) -> PortError {
    use br_runner_threads::Error;
    match error {
        Error::Refused(_) | Error::InvalidInput(_) => {
            PortError::new(PortErrorCode::ThreadsRefused, error.to_string())
        }
        other => PortError::new(PortErrorCode::ThreadsFailed, other.to_string()),
    }
}

pub(crate) fn refusal(error: impl std::fmt::Display) -> PortError {
    PortError::new(PortErrorCode::AdditionRefused, error.to_string())
}
