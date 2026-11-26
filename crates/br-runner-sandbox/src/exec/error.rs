#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecError {
    #[error("the exec stream of {operation} ended with no status")]
    NoStatus { operation: &'static str },
    #[error("{operation} ended with the exit code {code}")]
    Failed { operation: &'static str, code: i32 },
    #[error("the exec stream of {operation} failed: {reason}")]
    Io {
        operation: &'static str,
        reason: String,
    },
    #[error("{operation} did not end in the time that the configuration gives")]
    TimedOut { operation: &'static str },
    #[error("the file {path} does not exist or cannot be read")]
    Unreadable { path: String },
    #[error("{text:?} is not the process id of a process")]
    ProcessId { text: String },
    #[error("the output of {operation} is not the output that was expected")]
    Output { operation: &'static str },
}

impl ExecError {
    pub fn is_refusal(&self) -> bool {
        matches!(self, Self::Unreadable { .. } | Self::ProcessId { .. })
    }

    pub fn io(operation: &'static str, error: std::io::Error) -> Self {
        Self::Io {
            operation,
            reason: error.kind().to_string(),
        }
    }
}
