use std::time::Duration;

use br_agentic_harness_contract::{FreshReason, TurnId};

use crate::formats::{Manifest, Record, RecordState, Tombstone};
use crate::time::Timestamp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreView {
    Manifest(Box<Manifest>),
    Tombstone(Tombstone),
    Record(Record),
    Nothing,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartPlan {
    Restore(Box<Manifest>),
    Fresh(FreshReason),
}

pub fn plan_start(view: StoreView, parent: TurnId, now: Timestamp, ttl: Duration) -> StartPlan {
    match view {
        StoreView::Manifest(manifest) => {
            let expired_at = manifest.made_at().plus(ttl);
            if expired_at.has_passed(now) {
                StartPlan::Fresh(live_expired(expired_at))
            } else {
                StartPlan::Restore(manifest)
            }
        }
        StoreView::Tombstone(tombstone) => StartPlan::Fresh(live_expired(tombstone.expired_at())),
        StoreView::Record(record) => StartPlan::Fresh(match record.state() {
            RecordState::Done { .. } => FreshReason::SaveExpired { expired_at: None },
            RecordState::Started | RecordState::Failed { .. } => {
                FreshReason::SaveFailed { turn: parent }
            }
        }),
        StoreView::Nothing => StartPlan::Fresh(FreshReason::NoSaveEver),
        StoreView::Unavailable => StartPlan::Fresh(FreshReason::StoreUnavailable),
    }
}

pub fn live_expired(expired_at: Timestamp) -> FreshReason {
    FreshReason::SaveExpired {
        expired_at: Some(expired_at.to_system_time()),
    }
}

pub fn restore_failed(parent: TurnId) -> FreshReason {
    FreshReason::RestoreFailed { turn: parent }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreRecheck {
    Keep,
    Expired(FreshReason),
}

pub fn recheck_after_restore(
    first: &Manifest,
    second: Option<&Manifest>,
    now: Timestamp,
    ttl: Duration,
) -> RestoreRecheck {
    let still_in_time =
        second.is_some_and(|manifest| !manifest.made_at().plus(ttl).has_passed(now));
    if still_in_time {
        RestoreRecheck::Keep
    } else {
        RestoreRecheck::Expired(live_expired(first.made_at().plus(ttl)))
    }
}
