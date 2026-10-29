use std::num::NonZeroUsize;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("the maximum number of concurrent runs is below 1")]
    NoSlot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolkitSettings {
    pub max_concurrent_runs: NonZeroUsize,
    pub trigger_ack_wait: Duration,
    pub drain_timeout: Duration,
    pub cancel_grace: Duration,
    pub claim_wait: Duration,
    pub runner_version: String,
}

impl ToolkitSettings {
    pub fn new(
        max_concurrent_runs: usize,
        trigger_ack_wait: Duration,
        drain_timeout: Duration,
        cancel_grace: Duration,
        claim_wait: Duration,
        runner_version: impl Into<String>,
    ) -> Result<Self, SettingsError> {
        let max_concurrent_runs =
            NonZeroUsize::new(max_concurrent_runs).ok_or(SettingsError::NoSlot)?;
        Ok(Self {
            max_concurrent_runs,
            trigger_ack_wait,
            drain_timeout,
            cancel_grace,
            claim_wait,
            runner_version: runner_version.into(),
        })
    }
}
