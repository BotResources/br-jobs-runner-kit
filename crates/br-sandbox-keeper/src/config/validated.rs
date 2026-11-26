use std::collections::HashSet;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

use br_runner_sandbox::{LabelPrefix, MANAGED_TRUE, PodKeys};
use chrono_tz::Tz;
use url::Url;

use super::error::ConfigError;
use super::input::KeeperConfigInput;
use super::numbers::{Zero, seconds, small, text};
use super::pool::PoolConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StoreSettings {
    pub bucket: String,
    pub endpoint: Url,
    pub region: String,
    pub path_style: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LeaseSettings {
    pub namespace: Option<String>,
    pub duration: Duration,
    pub renew_deadline: Duration,
    pub retry_period: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeeperConfig {
    pub label_prefix: LabelPrefix,
    pub managed_namespace_selector: String,
    pub store: StoreSettings,
    pub listen_address: SocketAddr,
    pub save_ttl: Duration,
    pub skew_margin: Duration,
    pub sweep_interval: Duration,
    pub sweep_page_size: usize,
    pub attempt_ttl: Duration,
    pub tombstone_ttl: Duration,
    pub stuck_ttl: Duration,
    pub failed_pod_ttl: Duration,
    pub warm_max_age: Duration,
    pub pending_alarm: Duration,
    pub rate_window: Duration,
    pub rate_bucket: Duration,
    pub scale_down_cooldown: Duration,
    pub scale_down_step: u32,
    pub scale_down_step_interval: Duration,
    pub lease: LeaseSettings,
    pub timezone: Tz,
    pub pools: Vec<PoolConfig>,
}

fn store_endpoint(text: &str) -> Result<Url, ConfigError> {
    let invalid = |reason: &str| ConfigError::Invalid {
        key: "store_endpoint".to_owned(),
        reason: reason.to_owned(),
    };
    let url = Url::parse(text).map_err(|_| invalid("not a URL"))?;
    let plain = url.username().is_empty() && url.password().is_none();
    if matches!(url.scheme(), "http" | "https") && url.has_host() && plain {
        Ok(url)
    } else {
        Err(invalid("an http or https URL with a host and no user information"))
    }
}

fn pools(input: KeeperConfigInput) -> Result<Vec<PoolConfig>, ConfigError> {
    let inputs = super::numbers::required("pools", input.pools)?;
    if inputs.is_empty() {
        return Err(ConfigError::NoPools);
    }
    let pools = inputs
        .into_iter()
        .enumerate()
        .map(|(index, pool)| PoolConfig::from_input(index, pool))
        .collect::<Result<Vec<_>, _>>()?;
    let mut ids = HashSet::new();
    let mut sets = HashSet::new();
    for pool in &pools {
        if !ids.insert(pool.id.clone()) {
            return Err(ConfigError::DuplicatePoolId(pool.id.clone()));
        }
        if !sets.insert((pool.namespace.clone(), pool.replicaset.clone())) {
            return Err(ConfigError::DuplicateReplicaSet {
                namespace: pool.namespace.clone(),
                replicaset: pool.replicaset.clone(),
            });
        }
    }
    Ok(pools)
}

impl KeeperConfig {
    pub fn from_json(document: &str) -> Result<Self, ConfigError> {
        let input: KeeperConfigInput = serde_json::from_str(document)
            .map_err(|error| ConfigError::Document(error.to_string()))?;
        Self::try_from(input)
    }

    pub fn pod_keys(&self) -> PodKeys {
        PodKeys::new(self.label_prefix.clone())
    }

    pub fn pool(&self, namespace: &str, id: &str) -> Option<&PoolConfig> {
        self.pools
            .iter()
            .find(|pool| pool.namespace == namespace && pool.id == id)
    }
}

impl TryFrom<KeeperConfigInput> for KeeperConfig {
    type Error = ConfigError;

    fn try_from(input: KeeperConfigInput) -> Result<Self, ConfigError> {
        let label_prefix = match input.label_prefix.clone() {
            Some(prefix) => LabelPrefix::new(prefix).map_err(|error| ConfigError::Invalid {
                key: "label_prefix".to_owned(),
                reason: error.to_string(),
            })?,
            None => LabelPrefix::default(),
        };
        let managed_namespace_selector = match input.managed_namespace_selector.clone() {
            Some(selector) => text("managed_namespace_selector", Some(selector))?,
            None => format!("{}={MANAGED_TRUE}", PodKeys::new(label_prefix.clone()).managed()),
        };
        let store = StoreSettings {
            bucket: text("store_bucket", input.store_bucket.clone())?,
            endpoint: store_endpoint(&text("store_endpoint", input.store_endpoint.clone())?)?,
            region: text("store_region", input.store_region.clone())?,
            path_style: input.store_path_style.unwrap_or(false),
        };
        let listen_text = text("listen_address", input.listen_address.clone())?;
        let listen_address = SocketAddr::from_str(&listen_text).map_err(|_| ConfigError::Invalid {
            key: "listen_address".to_owned(),
            reason: "not a socket address".to_owned(),
        })?;
        let lease = LeaseSettings {
            namespace: input.lease_namespace.clone(),
            duration: seconds("lease_duration_seconds", input.lease_duration_seconds, Some(15), Zero::Forbidden)?,
            renew_deadline: seconds("renew_deadline_seconds", input.renew_deadline_seconds, Some(10), Zero::Forbidden)?,
            retry_period: seconds("retry_period_seconds", input.retry_period_seconds, Some(2), Zero::Forbidden)?,
        };
        if !(lease.duration > lease.renew_deadline && lease.renew_deadline > lease.retry_period) {
            return Err(ConfigError::LeaseOrder);
        }
        let timezone_text = input.timezone.clone().unwrap_or_else(|| "UTC".to_owned());
        let timezone = Tz::from_str(&timezone_text)
            .map_err(|_| ConfigError::UnknownTimezone(timezone_text))?;
        let rate_window = seconds("rate_window_seconds", input.rate_window_seconds, Some(300), Zero::Forbidden)?;
        let rate_bucket = seconds("rate_bucket_seconds", input.rate_bucket_seconds, Some(10), Zero::Forbidden)?;
        if rate_bucket > rate_window {
            return Err(ConfigError::BucketAboveWindow);
        }
        let scale_down_step = small("scale_down_step", input.scale_down_step, Some(5), Zero::Allowed)?;
        if scale_down_step < 1 {
            return Err(ConfigError::StepBelowOne);
        }
        Ok(Self {
            save_ttl: seconds("save_ttl_seconds", input.save_ttl_seconds, None, Zero::Forbidden)?,
            skew_margin: seconds("skew_margin_seconds", input.skew_margin_seconds, Some(60), Zero::Allowed)?,
            sweep_interval: seconds("sweep_interval_seconds", input.sweep_interval_seconds, Some(60), Zero::Forbidden)?,
            sweep_page_size: small("sweep_page_size", input.sweep_page_size, Some(1000), Zero::Forbidden)? as usize,
            attempt_ttl: seconds("attempt_ttl_seconds", input.attempt_ttl_seconds, Some(3600), Zero::Forbidden)?,
            tombstone_ttl: seconds("tombstone_ttl_seconds", input.tombstone_ttl_seconds, Some(31_536_000), Zero::Forbidden)?,
            stuck_ttl: seconds("stuck_ttl_seconds", input.stuck_ttl_seconds, Some(7200), Zero::Forbidden)?,
            failed_pod_ttl: seconds("failed_pod_ttl_seconds", input.failed_pod_ttl_seconds, Some(300), Zero::Forbidden)?,
            warm_max_age: seconds("warm_max_age_seconds", input.warm_max_age_seconds, Some(1800), Zero::Forbidden)?,
            pending_alarm: seconds("pending_alarm_seconds", input.pending_alarm_seconds, Some(60), Zero::Forbidden)?,
            scale_down_cooldown: seconds("scale_down_cooldown_seconds", input.scale_down_cooldown_seconds, Some(120), Zero::Forbidden)?,
            scale_down_step_interval: seconds("scale_down_step_interval_seconds", input.scale_down_step_interval_seconds, Some(10), Zero::Forbidden)?,
            pools: pools(input)?,
            label_prefix,
            managed_namespace_selector,
            store,
            listen_address,
            rate_window,
            rate_bucket,
            scale_down_step,
            lease,
            timezone,
        })
    }
}
