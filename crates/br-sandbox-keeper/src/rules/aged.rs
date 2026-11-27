use std::time::{Duration, UNIX_EPOCH};

use br_runner_sandbox::{FormatError, Record, Timestamp, Tombstone};
use uuid::Uuid;

use crate::bucket::ObjectInfo;
use crate::time::elapsed;

use super::store::SweepRules;

const TURN_VERSION: usize = 7;

fn turn_born_at(key: &str) -> Option<Timestamp> {
    let file = key.rsplit('/').next()?;
    let turn = Uuid::try_parse(file.strip_suffix(".json")?).ok()?;
    if turn.get_version_num() != TURN_VERSION {
        return None;
    }
    let (seconds, nanos) = turn.get_timestamp()?.to_unix();
    Some(Timestamp::from_system_time(
        UNIX_EPOCH + Duration::new(seconds, nanos),
    ))
}

fn may_be_old(info: &ObjectInfo, now: Timestamp, rules: &SweepRules, head_start: Duration) -> bool {
    turn_born_at(&info.key).is_none_or(|born| {
        elapsed(now, born) + rules.skew_margin >= head_start + rules.tombstone_ttl
    })
}

pub fn record_needs_read(info: &ObjectInfo, now: Timestamp, rules: &SweepRules) -> bool {
    may_be_old(info, now, rules, Duration::ZERO)
}

pub fn tombstone_needs_read(info: &ObjectInfo, now: Timestamp, rules: &SweepRules) -> bool {
    may_be_old(info, now, rules, rules.save_ttl)
}

pub fn record_is_old(
    bytes: &[u8],
    now: Timestamp,
    rules: &SweepRules,
) -> Result<bool, FormatError> {
    let record = Record::parse(bytes)?;
    Ok(record
        .age_anchor()
        .plus(rules.tombstone_ttl)
        .has_passed(now))
}

pub fn tombstone_is_old(
    bytes: &[u8],
    now: Timestamp,
    rules: &SweepRules,
) -> Result<bool, FormatError> {
    let tombstone = Tombstone::parse(bytes)?;
    Ok(tombstone
        .expired_at()
        .plus(rules.tombstone_ttl)
        .has_passed(now))
}

#[cfg(test)]
mod tests;
