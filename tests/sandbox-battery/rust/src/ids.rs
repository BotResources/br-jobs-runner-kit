use br_agentic_harness_contract::{AgentId, InstanceId, ThreadRef, TurnId, TurnRef};
use uuid::Uuid;

use crate::error::{Res, data};

pub fn thread() -> Res<ThreadRef> {
    ThreadRef::from_uuid(Uuid::now_v7()).map_err(|error| data("thread id", error.to_string()))
}

pub fn agent() -> Res<AgentId> {
    AgentId::from_uuid(Uuid::now_v7()).map_err(|error| data("agent id", error.to_string()))
}

pub fn turn() -> Res<TurnId> {
    TurnId::from_uuid(Uuid::now_v7()).map_err(|error| data("turn id", error.to_string()))
}

pub fn instance() -> Res<InstanceId> {
    InstanceId::new(Uuid::now_v7()).map_err(|error| data("instance id", error.to_string()))
}

#[derive(Debug, Clone, Copy)]
pub struct Lane {
    pub thread: ThreadRef,
    pub agent: AgentId,
}

impl Lane {
    pub fn new() -> Res<Self> {
        Ok(Self {
            thread: thread()?,
            agent: agent()?,
        })
    }

    pub fn with_agent(&self) -> Res<Self> {
        Ok(Self {
            thread: self.thread,
            agent: agent()?,
        })
    }

    pub fn turn(&self, id: TurnId, parent: Option<TurnId>) -> TurnRef {
        TurnRef::new(self.thread, self.agent, id, parent)
    }

    pub fn first(&self) -> Res<(TurnId, TurnRef)> {
        let id = turn()?;
        Ok((id, self.turn(id, None)))
    }

    pub fn child(&self, parent: TurnId) -> Res<(TurnId, TurnRef)> {
        let id = turn()?;
        Ok((id, self.turn(id, Some(parent))))
    }
}
