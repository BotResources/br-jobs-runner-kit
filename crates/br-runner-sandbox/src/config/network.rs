use std::collections::HashSet;

use br_agentic_harness_contract::Site;
use serde::Deserialize;

use super::error::ConfigError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkMode {
    None,
    Full,
    Allow,
}

impl NetworkMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Full => "full",
            Self::Allow => "allow",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkRule {
    None,
    Full,
    Allow(HashSet<Site>),
}

impl NetworkRule {
    pub fn new(mode: NetworkMode, allowlist: &[String]) -> Result<Self, ConfigError> {
        match (mode, allowlist.is_empty()) {
            (NetworkMode::Allow, true) => Err(ConfigError::AllowlistRequired),
            (NetworkMode::Allow, false) => allowlist
                .iter()
                .map(|text| Site::new(text.as_str()).map_err(|_| ConfigError::BlankSite))
                .collect::<Result<HashSet<_>, _>>()
                .map(Self::Allow),
            (mode, false) => Err(ConfigError::AllowlistForbidden {
                mode: mode.as_str(),
            }),
            (NetworkMode::None, true) => Ok(Self::None),
            (NetworkMode::Full, true) => Ok(Self::Full),
        }
    }

    pub fn mode(&self) -> NetworkMode {
        match self {
            Self::None => NetworkMode::None,
            Self::Full => NetworkMode::Full,
            Self::Allow(_) => NetworkMode::Allow,
        }
    }
}
