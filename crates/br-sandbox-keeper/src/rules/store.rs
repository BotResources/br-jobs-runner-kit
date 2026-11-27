use std::collections::BTreeMap;
use std::time::Duration;

use br_runner_sandbox::{
    FormatError, Manifest, Record, Timestamp, Tombstone, manifest_key, tombstone_key,
};

use crate::bucket::ObjectInfo;
use crate::time::elapsed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepRules {
    pub save_ttl: Duration,
    pub skew_margin: Duration,
    pub attempt_ttl: Duration,
    pub tombstone_ttl: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptListing {
    pub keys: Vec<String>,
    pub newest: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnListing {
    pub dir: String,
    pub manifest: Option<ObjectInfo>,
    pub attempts: BTreeMap<String, AttemptListing>,
}

enum Kind {
    Manifest,
    Part(String),
}

fn classify(key: &str) -> Option<(String, Kind)> {
    let segments: Vec<&str> = key.split('/').collect();
    if segments.first() != Some(&"saves") || segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    match segments.as_slice() {
        [_, _, _, _, _, "manifest.json"] => Some((segments[..5].join("/"), Kind::Manifest)),
        [_, _, _, _, _, attempt, file] if file.starts_with("part-") => {
            Some((segments[..5].join("/"), Kind::Part((*attempt).to_owned())))
        }
        _ => None,
    }
}

#[derive(Debug, Default)]
pub struct TurnGrouper {
    current: Option<TurnListing>,
}

impl TurnGrouper {
    pub fn push(&mut self, info: ObjectInfo) -> Option<TurnListing> {
        let (dir, kind) = classify(&info.key)?;
        let finished = match &self.current {
            Some(turn) if turn.dir != dir => self.current.take(),
            _ => None,
        };
        let turn = self.current.get_or_insert_with(|| TurnListing {
            dir,
            manifest: None,
            attempts: BTreeMap::new(),
        });
        match kind {
            Kind::Manifest => turn.manifest = Some(info),
            Kind::Part(attempt) => {
                let listing = turn
                    .attempts
                    .entry(attempt)
                    .or_insert_with(|| AttemptListing {
                        keys: Vec::new(),
                        newest: info.last_modified,
                    });
                listing.newest = listing.newest.max(info.last_modified);
                listing.keys.push(info.key);
            }
        }
        finished
    }

    pub fn finish(&mut self) -> Option<TurnListing> {
        self.current.take()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ManifestRead {
    Absent,
    Unreadable,
    Parsed(Box<Manifest>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StoreStep {
    PutTombstone { key: String, body: Vec<u8> },
    DeleteManifest { key: String },
    DeleteExpiredPart { key: String },
    DeleteOrphanAttempt { keys: Vec<String> },
}

pub fn save_expired(made_at: Timestamp, now: Timestamp, rules: &SweepRules) -> bool {
    elapsed(now, made_at) > rules.save_ttl + rules.skew_margin
}

pub fn tombstone_for(manifest: &Manifest, save_ttl: Duration) -> Tombstone {
    let made_at = manifest.made_at();
    Tombstone::new(manifest.key().turn(), made_at, made_at.plus(save_ttl))
}

fn attempt_is_old(attempt: &AttemptListing, now: Timestamp, rules: &SweepRules) -> bool {
    elapsed(now, attempt.newest) > rules.attempt_ttl
}

pub fn read_manifest(turn: &TurnListing, bytes: &[u8]) -> ManifestRead {
    let Some(listed) = &turn.manifest else {
        return ManifestRead::Absent;
    };
    match Manifest::parse(bytes) {
        Ok(manifest) if manifest_key(manifest.key()) == listed.key => {
            ManifestRead::Parsed(Box::new(manifest))
        }
        _ => ManifestRead::Unreadable,
    }
}

fn orphans(
    turn: &TurnListing,
    named: Option<&str>,
    now: Timestamp,
    rules: &SweepRules,
) -> Vec<StoreStep> {
    turn.attempts
        .iter()
        .filter(|(attempt, _)| Some(attempt.as_str()) != named)
        .filter(|(_, listing)| attempt_is_old(listing, now, rules))
        .map(|(_, listing)| StoreStep::DeleteOrphanAttempt {
            keys: listing.keys.clone(),
        })
        .collect()
}

fn expiry_steps(turn: &TurnListing, manifest: &Manifest, rules: &SweepRules) -> Vec<StoreStep> {
    let mut steps = Vec::new();
    if let Ok(body) = tombstone_for(manifest, rules.save_ttl).to_vec() {
        steps.push(StoreStep::PutTombstone {
            key: tombstone_key(manifest.key()),
            body,
        });
    } else {
        return steps;
    }
    if let Some(listed) = &turn.manifest {
        steps.push(StoreStep::DeleteManifest {
            key: listed.key.clone(),
        });
    }
    steps.extend(
        turn.attempts
            .values()
            .flat_map(|attempt| attempt.keys.iter())
            .map(|key| StoreStep::DeleteExpiredPart { key: key.clone() }),
    );
    steps
}

pub fn plan_turn(
    turn: &TurnListing,
    read: &ManifestRead,
    now: Timestamp,
    rules: &SweepRules,
) -> Vec<StoreStep> {
    match read {
        ManifestRead::Unreadable => Vec::new(),
        ManifestRead::Parsed(manifest) if save_expired(manifest.made_at(), now, rules) => {
            expiry_steps(turn, manifest, rules)
        }
        ManifestRead::Parsed(manifest) => {
            let named = manifest.attempt().to_string();
            orphans(turn, Some(&named), now, rules)
        }
        ManifestRead::Absent => orphans(turn, None, now, rules),
    }
}

pub fn needs_read(info: &ObjectInfo, now: Timestamp, rules: &SweepRules) -> bool {
    elapsed(now, info.last_modified) >= rules.tombstone_ttl
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
