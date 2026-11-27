use std::time::Duration;

use br_runner_sandbox::Timestamp;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;

use super::CONDITION_ACCEPTED;
use super::acceptance::Acceptance;
use super::conditions::{FAILING_TYPE, READY_TYPE, build, failing_fields, ready_fields};
use crate::convert::time_of;
use crate::resource::SandboxPoolStatus;
use crate::rules::PoolJudgement;
use crate::time::elapsed;

pub const STATUS_WRITE_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReplicaSetView {
    Absent,
    NotOwned,
    Owned { refusal: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Counts {
    pub desired: u32,
    pub replicas: u32,
    pub ready: u32,
    pub updated: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Observation {
    pub counts: Counts,
    pub replica_set: ReplicaSetView,
    pub failing: PoolJudgement,
    pub at: Timestamp,
}

impl Counts {
    pub fn new(desired: u32, replicas: u32, ready: u32, updated: u32) -> Self {
        Self {
            desired,
            replicas,
            ready,
            updated,
        }
    }
}

impl Observation {
    pub fn new(
        counts: Counts,
        replica_set: ReplicaSetView,
        failing: PoolJudgement,
        at: Timestamp,
    ) -> Self {
        Self {
            counts,
            replica_set,
            failing,
            at,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct StatusInput<'a> {
    pub generation: Option<i64>,
    pub min_replicas: Option<u32>,
    pub acceptance: &'a Acceptance,
    pub observation: Option<&'a Observation>,
    pub previous: Option<&'a SandboxPoolStatus>,
    pub now: Timestamp,
}

impl<'a> StatusInput<'a> {
    pub fn new(
        generation: Option<i64>,
        min_replicas: Option<u32>,
        acceptance: &'a Acceptance,
        observation: Option<&'a Observation>,
        previous: Option<&'a SandboxPoolStatus>,
        now: Timestamp,
    ) -> Self {
        Self {
            generation,
            min_replicas,
            acceptance,
            observation,
            previous,
            now,
        }
    }
}

fn count_of(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

pub fn compute_status(input: &StatusInput<'_>) -> SandboxPoolStatus {
    let previous = input.previous.cloned().unwrap_or_default();
    let (status, reason, message) = input.acceptance.fields();
    let accepted = build(
        CONDITION_ACCEPTED,
        super::conditions::Fields::new(status, reason, message),
        &previous.conditions,
        input.generation,
        input.now,
    );
    let ready = build(
        READY_TYPE,
        ready_fields(input.observation, input.min_replicas),
        &previous.conditions,
        input.generation,
        input.now,
    );
    let mut conditions = vec![accepted, ready];
    match input.observation {
        Some(observation) => conditions.push(build(
            FAILING_TYPE,
            failing_fields(observation),
            &previous.conditions,
            input.generation,
            input.now,
        )),
        None => conditions.extend(
            previous
                .conditions
                .iter()
                .filter(|condition| condition.type_ == FAILING_TYPE)
                .cloned(),
        ),
    }
    let judged = !matches!(input.acceptance, Acceptance::NotObserved);
    let mut next = SandboxPoolStatus {
        observed_generation: if judged {
            input.generation
        } else {
            previous.observed_generation
        },
        conditions,
        ..previous
    };
    if let Some(observation) = input.observation {
        next.desired_replicas = Some(count_of(observation.counts.desired));
        next.replicas = Some(count_of(observation.counts.replicas));
        next.ready_replicas = Some(count_of(observation.counts.ready));
        next.updated_replicas = Some(count_of(observation.counts.updated));
        next.observed_at = Some(time_of(observation.at));
    }
    next
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StatusWrite {
    Skip,
    Wait { until: Timestamp },
    Write(SandboxPoolStatus),
}

fn same_condition(a: &Condition, b: &Condition) -> bool {
    a.type_ == b.type_
        && a.status == b.status
        && a.reason == b.reason
        && a.message == b.message
        && a.observed_generation == b.observed_generation
}

fn same_values(a: &SandboxPoolStatus, b: &SandboxPoolStatus) -> bool {
    a.observed_generation == b.observed_generation
        && a.desired_replicas == b.desired_replicas
        && a.replicas == b.replicas
        && a.ready_replicas == b.ready_replicas
        && a.updated_replicas == b.updated_replicas
        && a.conditions.len() == b.conditions.len()
        && a.conditions
            .iter()
            .all(|left| b.conditions.iter().any(|right| same_condition(left, right)))
}

pub fn decide_write(
    next: SandboxPoolStatus,
    current: Option<&SandboxPoolStatus>,
    last_write: Option<Timestamp>,
    now: Timestamp,
) -> StatusWrite {
    if current.is_some_and(|current| same_values(&next, current)) {
        return StatusWrite::Skip;
    }
    match last_write {
        Some(last) if elapsed(now, last) < STATUS_WRITE_INTERVAL => StatusWrite::Wait {
            until: last.plus(STATUS_WRITE_INTERVAL),
        },
        _ => StatusWrite::Write(next),
    }
}
