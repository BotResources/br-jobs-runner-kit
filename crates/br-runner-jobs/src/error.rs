#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Cause {
    WaitFired,
    Transport,
    NoListener,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Failure {
    pub cause: Cause,
    pub text: String,
}

impl Failure {
    pub(crate) fn new(cause: Cause, text: impl Into<String>) -> Self {
        Self {
            cause,
            text: text.into(),
        }
    }

    pub(crate) fn wait(text: impl Into<String>) -> Self {
        Self::new(Cause::WaitFired, text)
    }

    pub(crate) fn transport(text: impl Into<String>) -> Self {
        Self::new(Cause::Transport, text)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cause = match self.cause {
            Cause::WaitFired => "the wait fired",
            Cause::Transport => "the transport failed",
            Cause::NoListener => "no listener",
        };
        write!(f, "{cause}: {}", self.text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CredentialKind {
    NoUsable,
    FetchFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
#[error("{}: {text}", match .kind {
    CredentialKind::NoUsable => "no usable credential",
    CredentialKind::FetchFailed => "credential fetch failed",
})]
pub struct CredentialFailure {
    pub kind: CredentialKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Operation {
    Claim,
    StartedFact,
    Create,
    Finish,
    Cancel,
    Mail,
    Registration,
    ReadMarker,
    AgentList,
    Upload,
    Payload,
    Result,
    SavedState,
    Attachment,
    Read,
    Size,
}

impl Operation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claim => "claim",
            Self::StartedFact => "started fact",
            Self::Create => "create",
            Self::Finish => "finish",
            Self::Cancel => "cancel",
            Self::Mail => "mail",
            Self::Registration => "registration",
            Self::ReadMarker => "read marker",
            Self::AgentList => "agent list",
            Self::Upload => "upload",
            Self::Payload => "payload",
            Self::Result => "result",
            Self::SavedState => "saved state",
            Self::Attachment => "attachment",
            Self::Read => "read",
            Self::Size => "size",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refusal {
    pub operation: Operation,
    pub reason: String,
    pub message: String,
    pub retry_after_ms: Option<u64>,
}

impl Refusal {
    pub(crate) fn new(
        operation: Operation,
        reason: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            reason: reason.into(),
            message: message.into(),
            retry_after_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("{0}")]
    Credential(CredentialFailure),
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("unavailable, {0}")]
    Unavailable(Failure),
    #[error("lost, {0}")]
    Lost(Failure),
    #[error("refused: {}: {}: {}", .0.operation.as_str(), .0.reason, .0.message)]
    Refused(Refusal),
    #[error("infra missing: {0}")]
    InfraMissing(String),
    #[error("unreadable: {0}")]
    Unreadable(String),
    #[error("the agent book was dropped")]
    BookDropped,
}

impl Error {
    pub(crate) fn invalid(text: impl Into<String>) -> Self {
        Self::InvalidInput(text.into())
    }

    pub(crate) fn unavailable(text: impl Into<String>) -> Self {
        Self::Unavailable(Failure::transport(text))
    }

    pub(crate) fn lost(text: impl Into<String>) -> Self {
        Self::Lost(Failure::transport(text))
    }

    pub(crate) fn wait_fired(text: impl Into<String>) -> Self {
        Self::Lost(Failure::wait(text))
    }

    pub(crate) fn refused(
        operation: Operation,
        reason: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::Refused(Refusal::new(operation, reason, message))
    }

    pub fn is_refusal(&self) -> bool {
        matches!(self, Self::Refused(_))
    }
}
