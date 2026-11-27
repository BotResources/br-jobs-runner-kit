use serde::{Deserialize, Deserializer};
use serde_json::Value;

fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct KeeperConfigInput {
    pub label_prefix: Option<String>,
    pub managed_namespace_selector: Option<String>,
    pub store_bucket: Option<String>,
    pub store_endpoint: Option<String>,
    pub store_region: Option<String>,
    pub store_path_style: Option<bool>,
    pub listen_address: Option<String>,
    pub save_ttl_seconds: Option<i64>,
    pub skew_margin_seconds: Option<i64>,
    pub sweep_interval_seconds: Option<i64>,
    pub sweep_page_size: Option<i64>,
    pub attempt_ttl_seconds: Option<i64>,
    pub tombstone_ttl_seconds: Option<i64>,
    pub stuck_ttl_seconds: Option<i64>,
    pub failed_pod_ttl_seconds: Option<i64>,
    pub warm_max_age_seconds: Option<i64>,
    pub pending_alarm_seconds: Option<i64>,
    pub rate_window_seconds: Option<i64>,
    pub rate_bucket_seconds: Option<i64>,
    pub scale_down_cooldown_seconds: Option<i64>,
    pub scale_down_step: Option<i64>,
    pub scale_down_step_interval_seconds: Option<i64>,
    pub lease_namespace: Option<String>,
    pub lease_duration_seconds: Option<i64>,
    pub renew_deadline_seconds: Option<i64>,
    pub retry_period_seconds: Option<i64>,
    pub timezone: Option<String>,
    pub idle_ttl_seconds: Option<i64>,
    pub idle_keep_min_state_bytes: Option<i64>,
    pub agent_container: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub pools: Option<Value>,
}
