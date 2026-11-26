use std::time::Duration;

use super::error::ConfigError;
use super::input::{FloorInput, PoolInput};
use super::numbers::{Zero, count, required, seconds, small, text};

const DAY_SECONDS: u32 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FloorWindow {
    pub from_second: u32,
    pub to_second: u32,
    pub floor: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PoolConfig {
    pub id: String,
    pub namespace: String,
    pub replicaset: String,
    pub min_replicas: u32,
    pub max_replicas: u32,
    pub refill: Duration,
    pub margin: u32,
    pub floors: Vec<FloorWindow>,
    pub idle_ttl: Duration,
    pub idle_keep_min_state_bytes: u64,
}

fn time_of_day(key: &str, text: &str, allow_midnight_end: bool) -> Result<u32, ConfigError> {
    let invalid = |reason: &str| ConfigError::Invalid {
        key: key.to_owned(),
        reason: reason.to_owned(),
    };
    let parts: Vec<&str> = text.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return Err(invalid("a time of day is HH:MM or HH:MM:SS"));
    }
    let mut numbers = [0u32; 3];
    for (slot, part) in numbers.iter_mut().zip(&parts) {
        if part.len() != 2 || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid("a time of day is HH:MM or HH:MM:SS"));
        }
        *slot = part.parse().map_err(|_| invalid("not a number"))?;
    }
    let [hours, minutes, seconds] = numbers;
    if minutes > 59 || seconds > 59 {
        return Err(invalid("minutes and seconds are below 60"));
    }
    let total = hours * 3600 + minutes * 60 + seconds;
    if total == DAY_SECONDS && allow_midnight_end {
        return Ok(total);
    }
    if hours > 23 {
        return Err(invalid("hours are below 24 (24:00 ends a window only)"));
    }
    Ok(total)
}

fn floor_window(pool: &str, index: usize, input: FloorInput) -> Result<FloorWindow, ConfigError> {
    let key = |name: &str| format!("pools[{pool}].floors[{index}].{name}");
    let from = time_of_day(&key("from"), &text(&key("from"), input.from)?, false)?;
    let to = time_of_day(&key("to"), &text(&key("to"), input.to)?, true)?;
    if to <= from {
        return Err(ConfigError::WindowEndsNoLater {
            pool: pool.to_owned(),
            index,
        });
    }
    let floor = small(&key("floor"), input.floor, None, Zero::Allowed)?;
    Ok(FloorWindow {
        from_second: from,
        to_second: to,
        floor,
    })
}

impl PoolConfig {
    pub(super) fn from_input(index: usize, input: PoolInput) -> Result<Self, ConfigError> {
        let key = |name: &str| format!("pools[{index}].{name}");
        let id = text(&key("id"), input.id)?;
        let namespace = text(&key("namespace"), input.namespace)?;
        let replicaset = text(&key("replicaset"), input.replicaset)?;
        let min_replicas = small(
            &key("min_replicas"),
            input.min_replicas,
            None,
            Zero::Allowed,
        )?;
        let max_replicas = small(
            &key("max_replicas"),
            input.max_replicas,
            None,
            Zero::Allowed,
        )?;
        if min_replicas > max_replicas {
            return Err(ConfigError::MinAboveMax { pool: id });
        }
        let floors = required(&key("floors"), input.floors.or(Some(Vec::new())))?
            .into_iter()
            .enumerate()
            .map(|(position, floor)| floor_window(&id, position, floor))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            refill: seconds(
                &key("refill_seconds"),
                input.refill_seconds,
                Some(3),
                Zero::Allowed,
            )?,
            margin: small(&key("margin"), input.margin, Some(2), Zero::Allowed)?,
            idle_ttl: seconds(
                &key("idle_ttl_seconds"),
                input.idle_ttl_seconds,
                Some(120),
                Zero::Allowed,
            )?,
            idle_keep_min_state_bytes: count(
                &key("idle_keep_min_state_bytes"),
                input.idle_keep_min_state_bytes,
                Some(10_000_000),
                Zero::Allowed,
            )?,
            id,
            namespace,
            replicaset,
            min_replicas,
            max_replicas,
            floors,
        })
    }
}
