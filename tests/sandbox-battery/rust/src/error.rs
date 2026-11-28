use std::fmt;

use br_agentic_harness_contract::PortError;

#[derive(thiserror::Error)]
pub enum BatteryError {
    #[error("the variable {name} is not set; it comes from {origin}")]
    MissingVariable {
        name: &'static str,
        origin: &'static str,
    },
    #[error("the variable {name} holds {value:?}, which is not {expected}")]
    BadVariable {
        name: &'static str,
        value: String,
        expected: &'static str,
    },
    #[error("the command `{command}` did not start: {reason}")]
    Spawn { command: String, reason: String },
    #[error("the command `{command}` ended with the code {code}: {detail}")]
    Command {
        command: String,
        code: i32,
        detail: String,
    },
    #[error("the command `{command}` did not end in {seconds} s")]
    CommandTimeout { command: String, seconds: u64 },
    #[error("{what} did not end in {seconds} s: hang after {seconds} s")]
    Hang { what: String, seconds: u64 },
    #[error("the file {path}: {reason}")]
    File { path: String, reason: String },
    #[error("the lock {path} was held by another test for more than {seconds} s")]
    LockBusy { path: String, seconds: u64 },
    #[error("the data of {what} is not as expected: {reason}")]
    Data { what: String, reason: String },
    #[error("the cluster is not reachable: {0}")]
    ClusterUnreachable(String),
    #[error("an expected state did not come in {seconds} s: {what}")]
    NotReached { what: String, seconds: u64 },
    #[error(transparent)]
    Adapter(#[from] br_runner_sandbox::Error),
    #[error(transparent)]
    Config(#[from] br_runner_sandbox::ConfigError),
    #[error(transparent)]
    Store(#[from] br_runner_sandbox::StoreError),
    #[error("the port gave: {0}")]
    Port(String),
    #[error("the run received a stop signal: no new change of the chart values starts")]
    Stopping,
    #[error("the test ended with no check recorded")]
    NoChecks,
    #[error("{count} check(s) failed: {names}")]
    Failed { count: usize, names: String },
}

impl fmt::Debug for BatteryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl From<PortError> for BatteryError {
    fn from(error: PortError) -> Self {
        Self::Port(format!("{error:?}"))
    }
}

impl From<std::io::Error> for BatteryError {
    fn from(error: std::io::Error) -> Self {
        Self::File {
            path: String::new(),
            reason: error.to_string(),
        }
    }
}

impl From<serde_json::Error> for BatteryError {
    fn from(error: serde_json::Error) -> Self {
        Self::Data {
            what: "json".to_owned(),
            reason: error.to_string(),
        }
    }
}

pub type Res<T = ()> = Result<T, BatteryError>;

pub fn data(what: impl Into<String>, reason: impl Into<String>) -> BatteryError {
    BatteryError::Data {
        what: what.into(),
        reason: reason.into(),
    }
}
