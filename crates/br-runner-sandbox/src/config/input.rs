use serde::Deserialize;

use super::network::NetworkMode;
use super::store::StoreCredentials;

const DEFAULT_SAVE_SCOPE: [&str; 2] = ["/work", "/home/agent"];

fn label_prefix() -> String {
    super::names::DEFAULT_LABEL_PREFIX.to_owned()
}

fn save_scope_paths() -> Vec<String> {
    DEFAULT_SAVE_SCOPE
        .iter()
        .map(|path| (*path).to_owned())
        .collect()
}

fn zstd_level() -> i32 {
    3
}

fn thirty() -> u64 {
    30
}

fn idle_ttl_seconds() -> u64 {
    120
}

fn idle_keep_min_state_bytes() -> u64 {
    10_000_000
}

fn claim_max_attempts() -> u32 {
    20
}

fn yes() -> bool {
    true
}

fn agent_container() -> String {
    "sandbox".to_owned()
}

fn seed_container() -> String {
    "seed".to_owned()
}

fn tools_path() -> String {
    "/opt/sbx/bin/bsdtar".to_owned()
}

fn seed_tools_path() -> String {
    "/sbx/bin/bsdtar".to_owned()
}

fn seed_root() -> String {
    "/skills".to_owned()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SandboxConfigInput {
    pub namespace: String,
    #[serde(default)]
    pub kube_context: Option<String>,
    pub client_id: String,
    #[serde(default = "label_prefix")]
    pub label_prefix: String,
    pub pool_id: String,
    pub network_mode: NetworkMode,
    #[serde(default)]
    pub allowlist: Vec<String>,
    pub save_ttl_seconds: u64,
    #[serde(default = "save_scope_paths")]
    pub save_scope_paths: Vec<String>,
    #[serde(default = "zstd_level")]
    pub save_zstd_level: i32,
    #[serde(default = "thirty")]
    pub restore_timeout_seconds: u64,
    #[serde(default = "thirty")]
    pub open_timeout_seconds: u64,
    #[serde(default)]
    pub exec_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub save_timeout_seconds: Option<u64>,
    #[serde(default = "idle_ttl_seconds")]
    pub idle_ttl_seconds: u64,
    #[serde(default = "idle_keep_min_state_bytes")]
    pub idle_keep_min_state_bytes: u64,
    #[serde(default = "claim_max_attempts")]
    pub claim_max_attempts: u32,
    #[serde(default = "yes")]
    pub cold_create: bool,
    #[serde(default = "agent_container")]
    pub agent_container: String,
    #[serde(default = "seed_container")]
    pub seed_container: String,
    #[serde(default = "tools_path")]
    pub tools_path: String,
    #[serde(default = "seed_tools_path")]
    pub seed_tools_path: String,
    #[serde(default = "seed_root")]
    pub seed_root: String,
    pub store_endpoint: String,
    pub store_bucket: String,
    pub store_region: String,
    #[serde(default)]
    pub store_path_style: bool,
    pub store_credentials: StoreCredentials,
}
