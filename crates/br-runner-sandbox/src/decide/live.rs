use std::time::Duration;

use br_agentic_harness_contract::TurnId;

use crate::config::SandboxConfig;
use crate::time::Timestamp;

use super::marks::PodMarks;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LivePolicy {
    pub save_ttl: Duration,
    pub idle_ttl: Duration,
    pub idle_keep_min_state_bytes: u64,
}

impl LivePolicy {
    pub fn of(config: &SandboxConfig) -> Self {
        Self {
            save_ttl: config.save_ttl(),
            idle_ttl: config.idle_ttl(),
            idle_keep_min_state_bytes: config.idle_keep_min_state_bytes(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveVerdict {
    NotCandidate,
    Expired { expired_at: Timestamp },
    Reusable,
    NotReusable,
}

pub fn judge_live_pod(
    marks: &PodMarks,
    parent: TurnId,
    seed_hash: &str,
    now: Timestamp,
    policy: &LivePolicy,
) -> LiveVerdict {
    let is_candidate = marks.clean == Some(true)
        && marks.state_turn.as_deref() == Some(parent.to_string().as_str());
    if !is_candidate {
        return LiveVerdict::NotCandidate;
    }
    let Some(saved_at) = marks.saved_at else {
        return LiveVerdict::NotReusable;
    };
    let expired_at = saved_at.plus(policy.save_ttl);
    if expired_at.has_passed(now) {
        return LiveVerdict::Expired { expired_at };
    }
    let idle_in_time = marks
        .idle_since
        .is_some_and(|idle_since| !idle_since.plus(policy.idle_ttl).has_passed(now));
    let heavy_enough = marks
        .state_bytes
        .is_some_and(|bytes| bytes >= policy.idle_keep_min_state_bytes);
    let same_seed = marks.seed_hash.as_deref() == Some(seed_hash);
    if idle_in_time && heavy_enough && same_seed {
        LiveVerdict::Reusable
    } else {
        LiveVerdict::NotReusable
    }
}
