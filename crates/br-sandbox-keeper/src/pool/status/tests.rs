mod acceptance;
mod conditions;
mod write;

use br_runner_sandbox::Timestamp;

use super::*;
use crate::resource::SandboxPoolStatus;
use crate::rules::PoolJudgement;
use crate::time::at;

pub(super) fn judgement(failing: bool) -> PoolJudgement {
    if failing {
        PoolJudgement {
            failing: true,
            reason: "ImagePullFailing",
            message: "2 warm pod(s) failing; kubelet reason: ImagePullBackOff".to_owned(),
        }
    } else {
        PoolJudgement {
            failing: false,
            reason: "NoWarmPodFailing",
            message: String::new(),
        }
    }
}

pub(super) fn observed(ready: u32, at_second: u64) -> Observation {
    Observation::new(
        Counts::new(4, 5, ready, 3),
        ReplicaSetView::Owned { refusal: None },
        judgement(false),
        at(at_second),
    )
}

pub(super) fn status_of(
    acceptance: &Acceptance,
    observation: Option<&Observation>,
    previous: Option<&SandboxPoolStatus>,
    now: Timestamp,
) -> SandboxPoolStatus {
    compute_status(&StatusInput::new(
        Some(7),
        Some(2),
        acceptance,
        observation,
        previous,
        now,
    ))
}

pub(super) fn condition<'a>(
    status: &'a SandboxPoolStatus,
    kind: &str,
) -> &'a k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition {
    status
        .conditions
        .iter()
        .find(|condition| condition.type_ == kind)
        .unwrap_or_else(|| panic!("no condition {kind}"))
}
