use std::fmt;

use serde::Deserialize;
use url::Url;

use super::error::ConfigError;

#[derive(Clone, PartialEq, Eq)]
pub struct StoreCredentials {
    access_key_id: String,
    secret_access_key: String,
}

impl StoreCredentials {
    pub fn new(
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Result<Self, ConfigError> {
        let access_key_id = access_key_id.into();
        let secret_access_key = secret_access_key.into();
        if access_key_id.trim().is_empty() || secret_access_key.trim().is_empty() {
            return Err(ConfigError::BlankCredentials);
        }
        Ok(Self {
            access_key_id,
            secret_access_key,
        })
    }

    pub fn access_key_id(&self) -> &str {
        &self.access_key_id
    }

    pub fn secret_access_key(&self) -> &str {
        &self.secret_access_key
    }
}

impl fmt::Debug for StoreCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StoreCredentials(<redacted>)")
    }
}

impl fmt::Display for StoreCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[derive(Deserialize)]
struct CredentialsWire {
    access_key_id: String,
    secret_access_key: String,
}

impl<'de> Deserialize<'de> for StoreCredentials {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = CredentialsWire::deserialize(deserializer)?;
        Self::new(wire.access_key_id, wire.secret_access_key).map_err(serde::de::Error::custom)
    }
}

pub fn store_endpoint(text: &str) -> Result<Url, ConfigError> {
    let url = Url::parse(text).map_err(|_| ConfigError::StoreEndpoint)?;
    let plain = url.username().is_empty() && url.password().is_none();
    if matches!(url.scheme(), "http" | "https") && url.has_host() && plain {
        Ok(url)
    } else {
        Err(ConfigError::StoreEndpoint)
    }
}
