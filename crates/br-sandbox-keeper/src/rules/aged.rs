use br_runner_sandbox::{FormatError, Record, Timestamp, Tombstone};

use super::store::SweepRules;

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
