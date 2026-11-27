mod acceptance;
mod conditions;
mod decide;
#[cfg(test)]
mod tests;

pub use acceptance::{Acceptance, judge_acceptance};
pub use decide::{
    Counts, Observation, ReplicaSetView, STATUS_WRITE_INTERVAL, StatusInput, StatusWrite,
    compute_status, decide_write,
};

pub const CONDITION_ACCEPTED: &str = "Accepted";
pub const CONDITION_READY: &str = "Ready";
pub const REASON_VALID: &str = "Valid";
pub const REASON_NOT_OBSERVED: &str = "NotObserved";
pub const REASON_INVALID_SPEC: &str = "InvalidSpec";
pub const REASON_NAMESPACE_NOT_FENCED: &str = "NamespaceNotFenced";
pub const REASON_DUPLICATE_POOL: &str = "DuplicatePool";
pub const REASON_TEMPLATE_ABSENT: &str = "TemplateAbsent";
pub const REASON_INVALID_TEMPLATE: &str = "InvalidTemplate";
pub const REASON_REPLICASET_ABSENT: &str = "ReplicaSetAbsent";
pub const REASON_REPLICASET_NOT_OWNED: &str = "ReplicaSetNotOwned";
pub const REASON_WARM_BELOW_MINIMUM: &str = "WarmBelowMinimum";
pub const REASON_PODS_REFUSED: &str = "PodsRefused";
pub const REASON_WARM_PODS_READY: &str = "WarmPodsReady";
