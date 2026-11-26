use std::fmt;

use super::error::ConfigError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AbsolutePath(String);

impl AbsolutePath {
    pub fn new(key: &'static str, text: &str) -> Result<Self, ConfigError> {
        if !text.starts_with('/') {
            return Err(ConfigError::NotAbsolute {
                key,
                value: text.to_owned(),
            });
        }
        if text.contains('\0') {
            return Err(ConfigError::PathEscapes {
                key,
                value: text.to_owned(),
            });
        }
        let mut parts = Vec::new();
        for part in text.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    return Err(ConfigError::PathEscapes {
                        key,
                        value: text.to_owned(),
                    });
                }
                other => parts.push(other),
            }
        }
        Ok(Self(format!("/{}", parts.join("/"))))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn relative(&self) -> &str {
        self.0.trim_start_matches('/')
    }

    pub fn holds(&self, other: &AbsolutePath) -> bool {
        self.0 == other.0
            || self.0 == "/"
            || other
                .0
                .strip_prefix(self.0.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

impl fmt::Display for AbsolutePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
