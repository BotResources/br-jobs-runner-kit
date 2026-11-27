use br_runner_sandbox::Timestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PoolState {
    Warm,
    Claimed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Phase {
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Mark<T> {
    Absent,
    Unreadable,
    Value(T),
}

impl<T> Mark<T> {
    pub fn is_absent(&self) -> bool {
        matches!(self, Mark::Absent)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Terminated {
    pub exit_code: i32,
    pub reason: Option<String>,
}

impl Terminated {
    pub fn new(exit_code: i32, reason: Option<String>) -> Self {
        Self { exit_code, reason }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ContainerFacts {
    pub waiting_reason: Option<String>,
    pub terminated: Option<Terminated>,
}

impl ContainerFacts {
    pub fn waiting(reason: impl Into<String>) -> Self {
        Self {
            waiting_reason: Some(reason.into()),
            terminated: None,
        }
    }

    pub fn terminated(terminated: Terminated) -> Self {
        Self {
            waiting_reason: None,
            terminated: Some(terminated),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PodVersion {
    pub uid: String,
    pub resource_version: String,
}

impl PodVersion {
    pub fn new(uid: impl Into<String>, resource_version: impl Into<String>) -> Self {
        Self {
            uid: uid.into(),
            resource_version: resource_version.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PodFacts {
    pub namespace: String,
    pub name: String,
    pub uid: String,
    pub resource_version: String,
    pub pool_state: PoolState,
    pub pool_id: Option<String>,
    pub thread: Option<String>,
    pub agent: Option<String>,
    pub phase: Phase,
    pub created_at: Timestamp,
    pub ready: bool,
    pub been_ready: bool,
    pub owned: bool,
    pub deleting: bool,
    pub clean: Mark<bool>,
    pub claimed_at: Mark<Timestamp>,
    pub idle_since: Mark<Timestamp>,
    pub state_bytes: Mark<u64>,
    pub containers: Vec<ContainerFacts>,
}

impl PodFacts {
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        uid: impl Into<String>,
        resource_version: impl Into<String>,
        pool_state: PoolState,
        created_at: Timestamp,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            uid: uid.into(),
            resource_version: resource_version.into(),
            pool_state,
            pool_id: None,
            thread: None,
            agent: None,
            phase: Phase::Unknown,
            created_at,
            ready: false,
            been_ready: false,
            owned: false,
            deleting: false,
            clean: Mark::Absent,
            claimed_at: Mark::Absent,
            idle_since: Mark::Absent,
            state_bytes: Mark::Absent,
            containers: Vec::new(),
        }
    }

    pub fn version(&self) -> PodVersion {
        PodVersion {
            uid: self.uid.clone(),
            resource_version: self.resource_version.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConditionFacts {
    pub kind: String,
    pub status: String,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub last_transition_time: Option<Timestamp>,
}

impl ConditionFacts {
    pub fn new(kind: impl Into<String>, status: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            status: status.into(),
            reason: None,
            message: None,
            last_transition_time: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReplicaSetFacts {
    pub uid: String,
    pub replicas: Option<i32>,
    pub conditions: Vec<ConditionFacts>,
}

impl ReplicaSetFacts {
    pub fn new(uid: impl Into<String>, replicas: Option<i32>) -> Self {
        Self {
            uid: uid.into(),
            replicas,
            conditions: Vec::new(),
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use crate::time::at;

    pub fn warm(name: &str, created: u64) -> PodFacts {
        PodFacts {
            namespace: "sbx".to_owned(),
            name: name.to_owned(),
            uid: format!("uid-{name}"),
            resource_version: "1".to_owned(),
            pool_state: PoolState::Warm,
            pool_id: Some("p".to_owned()),
            thread: None,
            agent: None,
            phase: Phase::Running,
            created_at: at(created),
            ready: true,
            been_ready: false,
            owned: true,
            deleting: false,
            clean: Mark::Absent,
            claimed_at: Mark::Absent,
            idle_since: Mark::Absent,
            state_bytes: Mark::Absent,
            containers: Vec::new(),
        }
    }

    pub fn claimed(name: &str, clean: bool, claimed_at: u64) -> PodFacts {
        PodFacts {
            pool_state: PoolState::Claimed,
            owned: false,
            thread: Some("thread".to_owned()),
            agent: Some("agent".to_owned()),
            clean: Mark::Value(clean),
            claimed_at: Mark::Value(at(claimed_at)),
            ..warm(name, 0)
        }
    }

    pub fn waiting(reason: &str) -> ContainerFacts {
        ContainerFacts {
            waiting_reason: Some(reason.to_owned()),
            terminated: None,
        }
    }

    pub fn terminated(exit_code: i32, reason: &str) -> ContainerFacts {
        ContainerFacts {
            waiting_reason: None,
            terminated: Some(Terminated {
                exit_code,
                reason: Some(reason.to_owned()),
            }),
        }
    }
}
