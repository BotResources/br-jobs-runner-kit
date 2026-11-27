use std::time::Duration;

use br_runner_sandbox::Timestamp;

use crate::facts::{Mark, Phase, PodFacts, PoolState};

use super::failing::pod_failure;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExpiryRules {
    pub warm_max_age: Duration,
    pub stuck_ttl: Duration,
    pub failed_pod_ttl: Duration,
    pub pending_alarm: Duration,
    pub idle_ttl: Duration,
    pub idle_keep_min_state_bytes: u64,
}

impl ExpiryRules {
    pub fn new(
        warm_max_age: Duration,
        stuck_ttl: Duration,
        failed_pod_ttl: Duration,
        pending_alarm: Duration,
        idle_ttl: Duration,
        idle_keep_min_state_bytes: u64,
    ) -> Self {
        Self {
            warm_max_age,
            stuck_ttl,
            failed_pod_ttl,
            pending_alarm,
            idle_ttl,
            idle_keep_min_state_bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeleteReason {
    WarmMaxAge,
    Idle,
    Stuck,
    Failed,
    Rollout,
}

impl DeleteReason {
    pub fn label(self) -> &'static str {
        match self {
            DeleteReason::WarmMaxAge => "warm_max_age",
            DeleteReason::Idle => "idle",
            DeleteReason::Stuck => "stuck",
            DeleteReason::Failed => "failed",
            DeleteReason::Rollout => "rollout",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Action {
    Keep,
    Delete(DeleteReason),
    PendingAlarm,
    Unreadable(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Judgment {
    pub action: Action,
    pub recheck_at: Option<Timestamp>,
}

fn at_deadline(deadline: Timestamp, reason: DeleteReason, now: Timestamp) -> Judgment {
    if deadline.has_passed(now) {
        Judgment {
            action: Action::Delete(reason),
            recheck_at: None,
        }
    } else {
        Judgment {
            action: Action::Keep,
            recheck_at: Some(deadline),
        }
    }
}

fn unreadable(field: &'static str) -> Judgment {
    Judgment {
        action: Action::Unreadable(field),
        recheck_at: None,
    }
}

fn stuck(pod: &PodFacts, rules: &ExpiryRules, now: Timestamp) -> Judgment {
    match &pod.claimed_at {
        Mark::Value(claimed_at) => {
            at_deadline(claimed_at.plus(rules.stuck_ttl), DeleteReason::Stuck, now)
        }
        Mark::Absent | Mark::Unreadable => unreadable("claimed-at"),
    }
}

fn idle(pod: &PodFacts, idle_since: Timestamp, rules: &ExpiryRules, now: Timestamp) -> Judgment {
    match &pod.state_bytes {
        Mark::Value(bytes) => {
            let ttl = if *bytes >= rules.idle_keep_min_state_bytes {
                rules.idle_ttl
            } else {
                Duration::ZERO
            };
            at_deadline(idle_since.plus(ttl), DeleteReason::Idle, now)
        }
        Mark::Absent | Mark::Unreadable => unreadable("state-bytes"),
    }
}

fn claimed(pod: &PodFacts, rules: &ExpiryRules, now: Timestamp) -> Judgment {
    match &pod.clean {
        Mark::Absent | Mark::Unreadable => unreadable("clean"),
        Mark::Value(false) => stuck(pod, rules, now),
        Mark::Value(true) => match &pod.idle_since {
            Mark::Value(idle_since) => idle(pod, *idle_since, rules, now),
            Mark::Absent => stuck(pod, rules, now),
            Mark::Unreadable => unreadable("idle-since"),
        },
    }
}

fn warm(pod: &PodFacts, rules: &ExpiryRules, selected: bool, now: Timestamp) -> Judgment {
    if selected {
        return Judgment {
            action: Action::Delete(DeleteReason::Rollout),
            recheck_at: None,
        };
    }
    let pending = pod.phase == Phase::Pending;
    let held = pending && pod.unschedulable;
    let deadline = (!held).then(|| pod.created_at.plus(rules.warm_max_age));
    if deadline.is_some_and(|deadline| deadline.has_passed(now)) {
        return Judgment {
            action: Action::Delete(DeleteReason::WarmMaxAge),
            recheck_at: None,
        };
    }
    if !pending || pod_failure(pod).is_some() {
        return Judgment {
            action: Action::Keep,
            recheck_at: deadline,
        };
    }
    let alarm_at = pod.created_at.plus(rules.pending_alarm);
    if alarm_at.has_passed(now) {
        Judgment {
            action: Action::PendingAlarm,
            recheck_at: deadline,
        }
    } else {
        Judgment {
            action: Action::Keep,
            recheck_at: Some(deadline.map_or(alarm_at, |deadline| alarm_at.min(deadline))),
        }
    }
}

pub fn judge_pod(
    pod: &PodFacts,
    rules: &ExpiryRules,
    failed_since: Option<Timestamp>,
    rollout_selected: bool,
    now: Timestamp,
) -> Judgment {
    if pod.deleting {
        return Judgment {
            action: Action::Keep,
            recheck_at: None,
        };
    }
    if pod.phase == Phase::Failed {
        let seen = failed_since.unwrap_or(now);
        return at_deadline(seen.plus(rules.failed_pod_ttl), DeleteReason::Failed, now);
    }
    match pod.pool_state {
        PoolState::Warm => warm(pod, rules, rollout_selected, now),
        PoolState::Claimed => claimed(pod, rules, now),
    }
}

#[cfg(test)]
mod tests;
