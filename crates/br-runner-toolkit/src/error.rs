#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorCode {
    SettingMissing,
    SettingInvalid,
    JobsFailed,
    NotStarted,
    InstanceEnded,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code:?}: {reason}")]
pub struct Error {
    code: ErrorCode,
    reason: String,
}

impl Error {
    pub(crate) fn new(code: ErrorCode, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
        }
    }

    pub(crate) fn jobs(error: &br_runner_jobs::Error) -> Self {
        Self::new(ErrorCode::JobsFailed, error.to_string())
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}
