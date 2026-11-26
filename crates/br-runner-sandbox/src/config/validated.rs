use std::fmt;
use std::time::Duration;

use url::Url;

use crate::ids::{ClientId, InvalidClientId};

use super::error::ConfigError;
use super::input::SandboxConfigInput;
use super::names::{LabelPrefix, PoolId, plain};
use super::network::NetworkRule;
use super::path::AbsolutePath;
use super::store::{StoreCredentials, store_endpoint};

fn client_error(error: InvalidClientId) -> ConfigError {
    match error {
        InvalidClientId::Blank => ConfigError::Blank { key: "client_id" },
        InvalidClientId::Slash => ConfigError::HoldsSlash { key: "client_id" },
    }
}

fn above_zero(key: &'static str, seconds: u64) -> Result<Duration, ConfigError> {
    if seconds == 0 {
        Err(ConfigError::NotAboveZero { key })
    } else {
        Ok(Duration::from_secs(seconds))
    }
}

fn optional_above_zero(
    key: &'static str,
    seconds: Option<u64>,
) -> Result<Option<Duration>, ConfigError> {
    seconds.map(|seconds| above_zero(key, seconds)).transpose()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreSettings {
    endpoint: Url,
    bucket: String,
    region: String,
    path_style: bool,
    credentials: StoreCredentials,
}

impl StoreSettings {
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn region(&self) -> &str {
        &self.region
    }

    pub fn path_style(&self) -> bool {
        self.path_style
    }

    pub fn credentials(&self) -> &StoreCredentials {
        &self.credentials
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxConfig {
    namespace: String,
    kube_context: Option<String>,
    client_id: ClientId,
    label_prefix: LabelPrefix,
    pool_id: PoolId,
    network: NetworkRule,
    save_ttl: Duration,
    save_scope_paths: Vec<AbsolutePath>,
    save_zstd_level: i32,
    restore_timeout: Duration,
    open_timeout: Duration,
    exec_timeout: Option<Duration>,
    save_timeout: Option<Duration>,
    idle_ttl: Duration,
    idle_keep_min_state_bytes: u64,
    claim_max_attempts: u32,
    cold_create: bool,
    agent_container: String,
    seed_container: String,
    tools_path: AbsolutePath,
    seed_tools_path: AbsolutePath,
    seed_root: AbsolutePath,
    store: StoreSettings,
}

impl SandboxConfig {
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn kube_context(&self) -> Option<&str> {
        self.kube_context.as_deref()
    }

    pub fn client_id(&self) -> &ClientId {
        &self.client_id
    }

    pub fn label_prefix(&self) -> &LabelPrefix {
        &self.label_prefix
    }

    pub fn pool_id(&self) -> &PoolId {
        &self.pool_id
    }

    pub fn network(&self) -> &NetworkRule {
        &self.network
    }

    pub fn save_ttl(&self) -> Duration {
        self.save_ttl
    }

    pub fn save_scope_paths(&self) -> &[AbsolutePath] {
        &self.save_scope_paths
    }

    pub fn save_zstd_level(&self) -> i32 {
        self.save_zstd_level
    }

    pub fn restore_timeout(&self) -> Duration {
        self.restore_timeout
    }

    pub fn open_timeout(&self) -> Duration {
        self.open_timeout
    }

    pub fn exec_timeout(&self) -> Option<Duration> {
        self.exec_timeout
    }

    pub fn save_timeout(&self) -> Option<Duration> {
        self.save_timeout
    }

    pub fn idle_ttl(&self) -> Duration {
        self.idle_ttl
    }

    pub fn idle_keep_min_state_bytes(&self) -> u64 {
        self.idle_keep_min_state_bytes
    }

    pub fn claim_max_attempts(&self) -> u32 {
        self.claim_max_attempts
    }

    pub fn cold_create(&self) -> bool {
        self.cold_create
    }

    pub fn agent_container(&self) -> &str {
        &self.agent_container
    }

    pub fn seed_container(&self) -> &str {
        &self.seed_container
    }

    pub fn tools_path(&self) -> &AbsolutePath {
        &self.tools_path
    }

    pub fn seed_tools_path(&self) -> &AbsolutePath {
        &self.seed_tools_path
    }

    pub fn seed_root(&self) -> &AbsolutePath {
        &self.seed_root
    }

    pub fn store(&self) -> &StoreSettings {
        &self.store
    }
}

impl TryFrom<SandboxConfigInput> for SandboxConfig {
    type Error = ConfigError;

    fn try_from(input: SandboxConfigInput) -> Result<Self, ConfigError> {
        let save_scope_paths = scope(&input.save_scope_paths)?;
        let seed_root = AbsolutePath::new("seed_root", &input.seed_root)?;
        if let Some(inside) = save_scope_paths
            .iter()
            .find(|path| path.holds(&seed_root) || seed_root.holds(path))
        {
            return Err(ConfigError::SeedRootInScope {
                seed_root: seed_root.to_string(),
                scope_path: inside.to_string(),
            });
        }
        if !(1..=22).contains(&input.save_zstd_level) {
            return Err(ConfigError::ZstdLevel {
                level: input.save_zstd_level,
            });
        }
        if input.claim_max_attempts < 1 {
            return Err(ConfigError::ClaimAttempts);
        }
        Ok(Self {
            namespace: plain("namespace", input.namespace)?,
            kube_context: input
                .kube_context
                .map(|context| plain("kube_context", context))
                .transpose()?,
            client_id: ClientId::new(input.client_id).map_err(client_error)?,
            label_prefix: LabelPrefix::new(input.label_prefix)?,
            pool_id: PoolId::new(input.pool_id)?,
            network: NetworkRule::new(input.network_mode, &input.allowlist)?,
            save_ttl: above_zero("save_ttl_seconds", input.save_ttl_seconds)?,
            save_scope_paths,
            save_zstd_level: input.save_zstd_level,
            restore_timeout: above_zero("restore_timeout_seconds", input.restore_timeout_seconds)?,
            open_timeout: above_zero("open_timeout_seconds", input.open_timeout_seconds)?,
            exec_timeout: optional_above_zero("exec_timeout_seconds", input.exec_timeout_seconds)?,
            save_timeout: optional_above_zero("save_timeout_seconds", input.save_timeout_seconds)?,
            idle_ttl: Duration::from_secs(input.idle_ttl_seconds),
            idle_keep_min_state_bytes: input.idle_keep_min_state_bytes,
            claim_max_attempts: input.claim_max_attempts,
            cold_create: input.cold_create,
            agent_container: plain("agent_container", input.agent_container)?,
            seed_container: plain("seed_container", input.seed_container)?,
            tools_path: AbsolutePath::new("tools_path", &input.tools_path)?,
            seed_tools_path: AbsolutePath::new("seed_tools_path", &input.seed_tools_path)?,
            seed_root,
            store: StoreSettings {
                endpoint: store_endpoint(&input.store_endpoint)?,
                bucket: plain("store_bucket", input.store_bucket)?,
                region: plain("store_region", input.store_region)?,
                path_style: input.store_path_style,
                credentials: input.store_credentials,
            },
        })
    }
}

fn scope(texts: &[String]) -> Result<Vec<AbsolutePath>, ConfigError> {
    let paths = texts
        .iter()
        .map(|text| AbsolutePath::new("save_scope_paths", text))
        .collect::<Result<Vec<_>, _>>()?;
    for (index, outer) in paths.iter().enumerate() {
        for (other, inner) in paths.iter().enumerate() {
            if index != other && outer.holds(inner) {
                return Err(ConfigError::ScopeNested {
                    outer: outer.to_string(),
                    inner: inner.to_string(),
                });
            }
        }
    }
    if !paths.iter().any(|path| path.as_str() == "/work") {
        return Err(ConfigError::ScopeWithoutWork);
    }
    Ok(paths)
}

impl fmt::Display for SandboxConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sandbox adapter: namespace {}, client {}, pool {}, network {}",
            self.namespace,
            self.client_id,
            self.pool_id,
            self.network.mode().as_str()
        )
    }
}
