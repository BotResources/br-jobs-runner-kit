use br_jobs_runner_core::PresenceConfigError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HarnessError {
    #[error("connecting to NATS at {url}: {source}")]
    Connect {
        url: String,
        #[source]
        source: async_nats::ConnectError,
    },
    #[error("{context}: {message}")]
    Infra {
        context: &'static str,
        message: String,
    },
    #[error(
        "the presence bucket declares no max_age; TTL eviction is the crash signal, fix the provisioning"
    )]
    PresenceBucketWithoutTtl,
    #[error("deriving the heartbeat from the presence ttl: {0}")]
    Heartbeat(#[from] PresenceConfigError),
}

impl HarnessError {
    pub(crate) fn infra(context: &'static str, error: impl std::fmt::Display) -> Self {
        Self::Infra {
            context,
            message: error.to_string(),
        }
    }
}
