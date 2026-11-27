use crate::facts::{Mark, Phase, PodFacts, PoolState};

use super::failing::pod_failure;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Wait {
    CurrentPodNotReady,
    ReplacementsMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RolloutState {
    Settled,
    Waiting(Wait),
    Stopped,
    Step,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RolloutPlan {
    pub state: RolloutState,
    pub delete: Vec<String>,
    pub stale: u32,
    pub updated: u32,
    pub unreadable: Vec<String>,
}

fn count(pods: &[&PodFacts]) -> u32 {
    u32::try_from(pods.len()).unwrap_or(u32::MAX)
}

fn plan(
    state: RolloutState,
    delete: Vec<String>,
    stale: &[&PodFacts],
    current: &[&PodFacts],
    unreadable: Vec<String>,
) -> RolloutPlan {
    RolloutPlan {
        state,
        delete,
        stale: count(stale),
        updated: count(current),
        unreadable,
    }
}

pub fn plan_rollout<'a>(
    pods: impl IntoIterator<Item = &'a PodFacts>,
    current_hash: &str,
    step: u32,
    replicas: u32,
) -> RolloutPlan {
    let live: Vec<&PodFacts> = pods
        .into_iter()
        .filter(|pod| pod.pool_state == PoolState::Warm && !pod.deleting)
        .collect();
    let mut current = Vec::new();
    let mut stale = Vec::new();
    let mut unreadable = Vec::new();
    for pod in &live {
        match &pod.template_hash {
            Mark::Value(hash) if hash == current_hash => current.push(*pod),
            Mark::Value(_) => stale.push(*pod),
            Mark::Absent | Mark::Unreadable => unreadable.push(pod.name.clone()),
        }
    }
    unreadable.sort();
    let verdict = |state, delete| plan(state, delete, &stale, &current, unreadable.clone());
    if stale.is_empty() {
        return verdict(RolloutState::Settled, Vec::new());
    }
    if current.iter().any(|pod| pod_failure(pod).is_some()) {
        return verdict(RolloutState::Stopped, Vec::new());
    }
    if current.iter().any(|pod| !pod.ready || pod.phase == Phase::Failed) {
        return verdict(
            RolloutState::Waiting(Wait::CurrentPodNotReady),
            Vec::new(),
        );
    }
    if count(&live) < replicas {
        return verdict(
            RolloutState::Waiting(Wait::ReplacementsMissing),
            Vec::new(),
        );
    }
    let mut order = stale.clone();
    order.sort_by(|a, b| {
        (a.ready, a.created_at, &a.name).cmp(&(b.ready, b.created_at, &b.name))
    });
    let chosen: Vec<String> = order
        .into_iter()
        .take(step as usize)
        .map(|pod| pod.name.clone())
        .collect();
    if chosen.is_empty() {
        return verdict(RolloutState::Settled, chosen);
    }
    verdict(RolloutState::Step, chosen)
}

#[cfg(test)]
mod tests;
