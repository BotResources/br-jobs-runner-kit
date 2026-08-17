use std::fmt;
use std::num::NonZeroUsize;
use std::time::Duration;

use thiserror::Error;

const MAX_KEY_LEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum KeyError {
    #[error("key must not be empty")]
    Empty,
    #[error("key exceeds {MAX_KEY_LEN} characters: {0}")]
    TooLong(usize),
    #[error("key contains invalid character `{0}`; allowed: a-z, 0-9, `-`, `_`")]
    InvalidCharacter(char),
}

fn validate_segment(value: &str) -> Result<(), KeyError> {
    if value.is_empty() {
        return Err(KeyError::Empty);
    }
    if value.len() > MAX_KEY_LEN {
        return Err(KeyError::TooLong(value.len()));
    }
    match value
        .chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-' || *c == '_'))
    {
        Some(invalid) => Err(KeyError::InvalidCharacter(invalid)),
        None => Ok(()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunnerType(String);

impl RunnerType {
    pub fn new(value: impl Into<String>) -> Result<Self, KeyError> {
        let value = value.into();
        validate_segment(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RunnerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for RunnerType {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstanceKey(String);

impl InstanceKey {
    pub fn new(value: impl Into<String>) -> Result<Self, KeyError> {
        let value = value.into();
        validate_segment(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstanceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for InstanceKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub runner_type: RunnerType,
    pub instance_key: InstanceKey,
    pub max_concurrent_runs: NonZeroUsize,
    pub presence_ttl: Duration,
    pub heartbeat_interval: Duration,
    pub drain_timeout: Duration,
}

impl RunnerConfig {
    pub fn new(runner_type: RunnerType, instance_key: InstanceKey) -> Self {
        Self {
            runner_type,
            instance_key,
            max_concurrent_runs: NonZeroUsize::MIN,
            presence_ttl: Duration::from_secs(30),
            heartbeat_interval: Duration::from_secs(10),
            drain_timeout: Duration::from_secs(600),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_valid_segment_when_building_keys_then_accepted() {
        let runner_type = RunnerType::new("llm-agent_v2").expect("valid runner type");
        let instance = InstanceKey::new("pod-0").expect("valid instance key");
        assert_eq!(runner_type.as_str(), "llm-agent_v2");
        assert_eq!(instance.to_string(), "pod-0");
    }

    #[test]
    fn given_empty_segment_when_building_then_rejected() {
        assert_eq!(RunnerType::new("").unwrap_err(), KeyError::Empty);
    }

    #[test]
    fn given_subject_breaking_characters_when_building_then_rejected() {
        for hostile in ["a.b", "a b", "a*", "a>", "A", "é"] {
            let error = InstanceKey::new(hostile).unwrap_err();
            assert!(
                matches!(error, KeyError::InvalidCharacter(_)),
                "{hostile} must be rejected"
            );
        }
    }

    #[test]
    fn given_oversized_segment_when_building_then_rejected() {
        let oversized = "a".repeat(MAX_KEY_LEN + 1);
        assert_eq!(
            RunnerType::new(oversized).unwrap_err(),
            KeyError::TooLong(MAX_KEY_LEN + 1)
        );
    }

    #[test]
    fn given_new_config_when_defaulted_then_single_slot_and_heartbeat_below_ttl() {
        let config = RunnerConfig::new(
            RunnerType::new("agent").expect("valid"),
            InstanceKey::new("pod-0").expect("valid"),
        );
        assert_eq!(config.max_concurrent_runs.get(), 1);
        assert!(config.heartbeat_interval < config.presence_ttl);
    }
}
