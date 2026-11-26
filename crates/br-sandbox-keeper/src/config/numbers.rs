use std::time::Duration;

use super::error::ConfigError;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Zero {
    Forbidden,
    Allowed,
}

pub(super) fn required<T>(key: &str, value: Option<T>) -> Result<T, ConfigError> {
    value.ok_or_else(|| ConfigError::Missing {
        key: key.to_owned(),
    })
}

pub(super) fn text(key: &str, value: Option<String>) -> Result<String, ConfigError> {
    let value = required(key, value)?;
    if value.trim().is_empty() {
        return Err(ConfigError::Invalid {
            key: key.to_owned(),
            reason: "it is blank".to_owned(),
        });
    }
    Ok(value)
}

pub(super) fn count(
    key: &str,
    value: Option<i64>,
    default: Option<u64>,
    zero: Zero,
) -> Result<u64, ConfigError> {
    let value = match (value, default) {
        (Some(value), _) => value,
        (None, Some(default)) => return Ok(default),
        (None, None) => return required(key, None),
    };
    match (value, zero) {
        (value, Zero::Forbidden) if value <= 0 => Err(ConfigError::NotAboveZero {
            key: key.to_owned(),
        }),
        (value, Zero::Allowed) if value < 0 => Err(ConfigError::Negative {
            key: key.to_owned(),
        }),
        (value, _) => Ok(value as u64),
    }
}

pub(super) fn seconds(
    key: &str,
    value: Option<i64>,
    default: Option<u64>,
    zero: Zero,
) -> Result<Duration, ConfigError> {
    count(key, value, default, zero).map(Duration::from_secs)
}

pub(super) fn small(
    key: &str,
    value: Option<i64>,
    default: Option<u64>,
    zero: Zero,
) -> Result<u32, ConfigError> {
    let value = count(key, value, default, zero)?;
    u32::try_from(value).map_err(|_| ConfigError::Invalid {
        key: key.to_owned(),
        reason: "it is too large".to_owned(),
    })
}
