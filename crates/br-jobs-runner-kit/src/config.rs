use std::fmt;
use std::num::{NonZeroU32, NonZeroUsize};
use std::time::Duration;

use contract_jobs::runner::Capacity;
use contract_jobs::segment::{SegmentError, SubjectSegment};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerType(SubjectSegment);

impl RunnerType {
    pub fn new(value: impl AsRef<str>) -> Result<Self, SegmentError> {
        Ok(Self(SubjectSegment::runner_type(value)?))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub(crate) fn segment(&self) -> &SubjectSegment {
        &self.0
    }
}

impl fmt::Display for RunnerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for RunnerType {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceKey(SubjectSegment);

impl InstanceKey {
    pub fn new(value: impl AsRef<str>) -> Result<Self, SegmentError> {
        Ok(Self(SubjectSegment::instance_key(value)?))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub(crate) fn segment(&self) -> &SubjectSegment {
        &self.0
    }
}

impl fmt::Display for InstanceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for InstanceKey {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub nats_url: String,
    pub runner_type: RunnerType,
    pub instance_key: InstanceKey,
    pub runner_version: String,
    pub max_concurrent_runs: NonZeroU32,
    pub ack_wait: Duration,
    pub drain_timeout: Duration,
}

impl RunnerConfig {
    pub fn new(
        nats_url: impl Into<String>,
        runner_type: RunnerType,
        instance_key: InstanceKey,
        runner_version: impl Into<String>,
    ) -> Self {
        Self {
            nats_url: nats_url.into(),
            runner_type,
            instance_key,
            runner_version: runner_version.into(),
            max_concurrent_runs: NonZeroU32::MIN,
            ack_wait: Duration::from_secs(30),
            drain_timeout: Duration::from_secs(600),
        }
    }

    pub(crate) fn capacity(&self) -> Capacity {
        Capacity::try_from(self.max_concurrent_runs.get())
            .expect("a NonZeroU32 always satisfies the capacity floor")
    }

    pub(crate) fn slot_count(&self) -> NonZeroUsize {
        NonZeroUsize::new(self.max_concurrent_runs.get() as usize)
            .expect("a NonZeroU32 stays non-zero as usize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> RunnerConfig {
        RunnerConfig::new(
            "nats://localhost:4222",
            RunnerType::new("analyst").expect("valid runner type"),
            InstanceKey::new("pod-0").expect("valid instance key"),
            "1.2.3",
        )
    }

    #[test]
    fn given_contract_valid_segments_when_building_keys_then_accepted() {
        let runner_type = RunnerType::new("llm-agent_v2").expect("valid runner type");
        assert_eq!(runner_type.as_str(), "llm-agent_v2");
        assert_eq!(
            InstanceKey::new("pod-0")
                .expect("valid instance key")
                .to_string(),
            "pod-0"
        );
    }

    #[test]
    fn given_subject_breaking_segments_when_building_then_the_contract_rejects_them() {
        for hostile in ["", "a.b", "a b", "a*", "a>"] {
            assert!(
                RunnerType::new(hostile).is_err(),
                "{hostile:?} must be rejected"
            );
            assert!(
                InstanceKey::new(hostile).is_err(),
                "{hostile:?} must be rejected"
            );
        }
    }

    #[test]
    fn given_new_config_when_defaulted_then_single_slot_thirty_second_ack_wait() {
        let config = config();
        assert_eq!(config.max_concurrent_runs.get(), 1);
        assert_eq!(config.capacity().get(), 1);
        assert_eq!(config.ack_wait, Duration::from_secs(30));
        assert_eq!(config.runner_version, "1.2.3");
    }
}
