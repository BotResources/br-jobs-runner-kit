use std::time::Duration;

use crate::error::{Error, ErrorCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    runner_version: String,
    max_concurrent_runs: u32,
    ack_wait: Duration,
    drain_timeout: Duration,
    cancel_grace: Duration,
    claim_wait: Duration,
    outcome_wait: Duration,
    retry_wait: Duration,
}

impl Settings {
    pub fn builder() -> SettingsBuilder {
        SettingsBuilder::default()
    }

    pub fn runner_version(&self) -> &str {
        &self.runner_version
    }

    pub fn max_concurrent_runs(&self) -> u32 {
        self.max_concurrent_runs
    }

    pub fn ack_wait(&self) -> Duration {
        self.ack_wait
    }

    pub fn drain_timeout(&self) -> Duration {
        self.drain_timeout
    }

    pub fn cancel_grace(&self) -> Duration {
        self.cancel_grace
    }

    pub fn claim_wait(&self) -> Duration {
        self.claim_wait
    }

    pub fn outcome_wait(&self) -> Duration {
        self.outcome_wait
    }

    pub fn retry_wait(&self) -> Duration {
        self.retry_wait
    }
}

#[derive(Debug, Clone, Default)]
pub struct SettingsBuilder {
    runner_version: Option<String>,
    max_concurrent_runs: Option<u32>,
    ack_wait: Option<Duration>,
    drain_timeout: Option<Duration>,
    cancel_grace: Option<Duration>,
    claim_wait: Option<Duration>,
    outcome_wait: Option<Duration>,
    retry_wait: Option<Duration>,
}

fn missing(name: &str) -> Error {
    Error::new(
        ErrorCode::SettingMissing,
        format!("the setting {name} is missing"),
    )
}

fn invalid(name: &str, rule: &str) -> Error {
    Error::new(
        ErrorCode::SettingInvalid,
        format!("the setting {name} {rule}"),
    )
}

fn nonzero(name: &str, value: Option<Duration>) -> Result<Duration, Error> {
    let value = value.ok_or_else(|| missing(name))?;
    if value.is_zero() {
        return Err(invalid(name, "is zero"));
    }
    Ok(value)
}

impl SettingsBuilder {
    pub fn runner_version(mut self, version: impl Into<String>) -> Self {
        self.runner_version = Some(version.into());
        self
    }

    pub fn max_concurrent_runs(mut self, runs: u32) -> Self {
        self.max_concurrent_runs = Some(runs);
        self
    }

    pub fn ack_wait(mut self, wait: Duration) -> Self {
        self.ack_wait = Some(wait);
        self
    }

    pub fn drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = Some(timeout);
        self
    }

    pub fn cancel_grace(mut self, grace: Duration) -> Self {
        self.cancel_grace = Some(grace);
        self
    }

    pub fn claim_wait(mut self, wait: Duration) -> Self {
        self.claim_wait = Some(wait);
        self
    }

    pub fn outcome_wait(mut self, wait: Duration) -> Self {
        self.outcome_wait = Some(wait);
        self
    }

    pub fn retry_wait(mut self, wait: Duration) -> Self {
        self.retry_wait = Some(wait);
        self
    }

    pub fn build(self) -> Result<Settings, Error> {
        let runner_version = self
            .runner_version
            .ok_or_else(|| missing("runner_version"))?;
        let max_concurrent_runs = self
            .max_concurrent_runs
            .ok_or_else(|| missing("max_concurrent_runs"))?;
        if max_concurrent_runs < 1 {
            return Err(invalid("max_concurrent_runs", "is below 1"));
        }
        Ok(Settings {
            runner_version,
            max_concurrent_runs,
            ack_wait: nonzero("ack_wait", self.ack_wait)?,
            drain_timeout: self.drain_timeout.ok_or_else(|| missing("drain_timeout"))?,
            cancel_grace: self.cancel_grace.ok_or_else(|| missing("cancel_grace"))?,
            claim_wait: nonzero("claim_wait", self.claim_wait)?,
            outcome_wait: nonzero("outcome_wait", self.outcome_wait)?,
            retry_wait: nonzero("retry_wait", self.retry_wait)?,
        })
    }
}
