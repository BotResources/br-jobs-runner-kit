use br_runner_sandbox::{
    CONDITION_WARM_PODS_FAILING, REASON_IMAGE_PULL_FAILING, REASON_START_CRASHING, Timestamp,
};

use crate::facts::{ConditionFacts, ContainerFacts, PodFacts};

const PULL_REASONS: [&str; 3] = ["ErrImagePull", "ImagePullBackOff", "InvalidImageName"];
const CRASH_LOOP: &str = "CrashLoopBackOff";
const TERMINATED_FALLBACK: &str = "Error";
const NONE_FAILING: &str = "NoWarmPodFailing";
const STATUS_TRUE: &str = "True";
const STATUS_FALSE: &str = "False";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FailureKind {
    ImagePull,
    Crash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PodFailure {
    pub kind: FailureKind,
    pub kubelet_reason: String,
}

fn container_failure(container: &ContainerFacts) -> Option<PodFailure> {
    if let Some(reason) = container.waiting_reason.as_deref() {
        if PULL_REASONS.contains(&reason) {
            return Some(PodFailure {
                kind: FailureKind::ImagePull,
                kubelet_reason: reason.to_owned(),
            });
        }
        if reason == CRASH_LOOP {
            return Some(PodFailure {
                kind: FailureKind::Crash,
                kubelet_reason: reason.to_owned(),
            });
        }
    }
    container
        .terminated
        .as_ref()
        .filter(|terminated| terminated.exit_code != 0)
        .map(|terminated| PodFailure {
            kind: FailureKind::Crash,
            kubelet_reason: terminated
                .reason
                .clone()
                .unwrap_or_else(|| TERMINATED_FALLBACK.to_owned()),
        })
}

pub fn pod_failure(pod: &PodFacts) -> Option<PodFailure> {
    if pod.ready || pod.been_ready || pod.deleting {
        return None;
    }
    let failures: Vec<PodFailure> = pod
        .containers
        .iter()
        .filter_map(container_failure)
        .collect();
    failures
        .iter()
        .find(|failure| failure.kind == FailureKind::ImagePull)
        .or_else(|| failures.first())
        .cloned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PoolJudgement {
    pub failing: bool,
    pub reason: &'static str,
    pub message: String,
}

pub fn judge_pool<'a>(warm_pods: impl IntoIterator<Item = &'a PodFacts>) -> PoolJudgement {
    let mut failures: Vec<(&str, PodFailure)> = warm_pods
        .into_iter()
        .filter_map(|pod| pod_failure(pod).map(|failure| (pod.name.as_str(), failure)))
        .collect();
    failures.sort_by(|a, b| a.0.cmp(b.0));
    let count = failures.len();
    let pulling = failures
        .iter()
        .find(|(_, failure)| failure.kind == FailureKind::ImagePull);
    let shown = pulling.or_else(|| failures.first());
    match shown {
        None => PoolJudgement {
            failing: false,
            reason: NONE_FAILING,
            message: String::new(),
        },
        Some((_, failure)) => PoolJudgement {
            failing: true,
            reason: match failure.kind {
                FailureKind::ImagePull => REASON_IMAGE_PULL_FAILING,
                FailureKind::Crash => REASON_START_CRASHING,
            },
            message: format!(
                "{count} warm pod(s) failing; kubelet reason: {}",
                failure.kubelet_reason
            ),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConditionWrite {
    pub status: &'static str,
    pub reason: &'static str,
    pub message: String,
    pub last_transition_time: Timestamp,
}

pub fn failing_status(judged: &PoolJudgement) -> &'static str {
    if judged.failing {
        STATUS_TRUE
    } else {
        STATUS_FALSE
    }
}

pub fn condition_write(
    current: &[ConditionFacts],
    judged: &PoolJudgement,
    now: Timestamp,
) -> Option<ConditionWrite> {
    let status = failing_status(judged);
    let existing = current
        .iter()
        .find(|condition| condition.kind == CONDITION_WARM_PODS_FAILING);
    let unchanged = existing.is_some_and(|condition| {
        condition.status == status
            && condition.reason.as_deref() == Some(judged.reason)
            && condition.message.as_deref().unwrap_or_default() == judged.message
    });
    if unchanged {
        return None;
    }
    let last_transition_time = existing
        .filter(|condition| condition.status == status)
        .and_then(|condition| condition.last_transition_time)
        .unwrap_or(now);
    Some(ConditionWrite {
        status,
        reason: judged.reason,
        message: judged.message.clone(),
        last_transition_time,
    })
}

#[cfg(test)]
mod tests;
