use std::collections::HashMap;

use uuid::Uuid;

use crate::backend::Fact;
use crate::error::Error;
use crate::model::CallMode;
use crate::queue::write::{Removal, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DepKey {
    Started,
    Registration(Uuid),
    Upload(Uuid),
    Create(Uuid),
    Answer,
    SavedState,
}

#[derive(Debug, Clone)]
pub(crate) enum Ended {
    Delivered,
    Refused(Error),
    Failed,
    Depends,
    Unsent,
}

impl Ended {
    pub(crate) fn of(result: &Result<(), Removal>) -> Self {
        match result {
            Ok(()) => Self::Delivered,
            Err(Removal::Refused(error)) => Self::Refused(error.clone()),
            Err(Removal::Failed(_)) => Self::Failed,
            Err(Removal::DependsOnRemoved) => Self::Depends,
            Err(Removal::LeftUnsent) => Self::Unsent,
        }
    }
}

pub(crate) fn own_key(write: &Write) -> Option<DepKey> {
    match write {
        Write::Started { .. } => Some(DepKey::Started),
        Write::Registration { agent, .. } => Some(DepKey::Registration(agent.agent_id)),
        Write::Upload { job_id, .. } => Some(DepKey::Upload(*job_id)),
        Write::Create(create) => Some(DepKey::Create(create.job_id)),
        Write::Answer(_) => Some(DepKey::Answer),
        Write::SavedState(_) => Some(DepKey::SavedState),
        _ => None,
    }
}

pub(crate) fn depends_on(write: &Write) -> Vec<DepKey> {
    let mut keys = Vec::new();
    if !matches!(write, Write::Started { .. }) {
        keys.push(DepKey::Started);
    }
    match write {
        Write::Mail { mail } => keys.push(DepKey::Registration(mail.from_address)),
        Write::Marker { agent_id, .. } => keys.push(DepKey::Registration(*agent_id)),
        Write::Create(create) => {
            keys.push(DepKey::Upload(create.job_id));
            if let CallMode::Background { caller_address } = create.mode {
                keys.push(DepKey::Registration(caller_address));
            }
        }
        Write::Finish { job_id, .. } | Write::Cancel { job_id, .. } => {
            keys.push(DepKey::Create(*job_id));
        }
        Write::Fact(Fact::Completed { .. }, _) => {
            keys.push(DepKey::Answer);
            keys.push(DepKey::SavedState);
        }
        _ => {}
    }
    keys
}

pub(crate) fn blocked(write: &Write, ended: &HashMap<DepKey, Ended>) -> Option<Removal> {
    for key in depends_on(write) {
        match ended.get(&key) {
            None | Some(Ended::Delivered) => {}
            Some(Ended::Refused(error))
                if matches!(write, Write::Create(_)) && matches!(key, DepKey::Upload(_)) =>
            {
                return Some(Removal::Refused(error.clone()));
            }
            Some(_) => return Some(Removal::DependsOnRemoved),
        }
    }
    None
}
