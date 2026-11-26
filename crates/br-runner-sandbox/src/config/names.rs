use std::fmt;

use super::error::ConfigError;

pub const DEFAULT_LABEL_PREFIX: &str = "br.sandbox";

fn not_blank(key: &'static str, text: &str) -> Result<(), ConfigError> {
    if text.trim().is_empty() {
        Err(ConfigError::Blank { key })
    } else {
        Ok(())
    }
}

pub(super) fn plain(key: &'static str, text: String) -> Result<String, ConfigError> {
    not_blank(key, &text)?;
    Ok(text)
}

pub(super) fn no_slash(key: &'static str, text: String) -> Result<String, ConfigError> {
    not_blank(key, &text)?;
    if text.contains('/') {
        return Err(ConfigError::HoldsSlash { key });
    }
    Ok(text)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LabelPrefix(String);

impl LabelPrefix {
    pub fn new(text: impl Into<String>) -> Result<Self, ConfigError> {
        no_slash("label_prefix", text.into()).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for LabelPrefix {
    fn default() -> Self {
        Self(DEFAULT_LABEL_PREFIX.to_owned())
    }
}

impl fmt::Display for LabelPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PoolId(String);

impl PoolId {
    pub fn new(text: impl Into<String>) -> Result<Self, ConfigError> {
        plain("pool_id", text.into()).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn replica_set_name(&self) -> String {
        format!("warm-{}", self.0)
    }
}

impl fmt::Display for PoolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
