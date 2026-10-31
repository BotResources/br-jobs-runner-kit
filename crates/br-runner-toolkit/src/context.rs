use std::sync::{Arc, Weak};

use br_core_auth::Passport;
use br_runner_jobs::{
    AgentBook, CallerHandle, Content, DeliveryQueue, Error, LogLevel, MailHandle, Work, WriteEnd,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::calls::Calls;

#[derive(Clone)]
pub struct RunContext {
    work: Arc<Work>,
    queue: Weak<DeliveryQueue>,
    book: Weak<AgentBook>,
    calls: Calls,
}

fn ended() -> Error {
    Error::InvalidInput("the run has ended".to_owned())
}

impl RunContext {
    pub(crate) fn new(
        work: Arc<Work>,
        queue: &Arc<DeliveryQueue>,
        book: &Arc<AgentBook>,
        calls: Calls,
    ) -> Self {
        Self {
            work,
            queue: Arc::downgrade(queue),
            book: Arc::downgrade(book),
            calls,
        }
    }

    pub fn job_id(&self) -> Uuid {
        self.work.job_id()
    }

    pub fn run_id(&self) -> Uuid {
        self.work.run_id()
    }

    pub fn attempt(&self) -> u32 {
        self.work.attempt()
    }

    pub fn parent_job_id(&self) -> Uuid {
        self.work.parent_job_id()
    }

    pub fn initial_job_id(&self) -> Uuid {
        self.work.initial_job_id()
    }

    pub fn passport(&self) -> &Passport {
        self.work.passport()
    }

    pub fn agent_book(&self) -> Result<Arc<AgentBook>, Error> {
        self.book.upgrade().ok_or_else(ended)
    }

    pub fn caller_handle(&self) -> Result<CallerHandle, Error> {
        Ok(self.agent_book()?.caller_handle())
    }

    pub fn mail_handle(&self) -> MailHandle {
        self.work.mail_handle()
    }

    pub async fn payload(&self) -> Result<Content, Error> {
        self.work.payload().await
    }

    pub async fn log(
        &self,
        level: LogLevel,
        message: impl Into<String>,
        step_index: Option<u32>,
    ) -> Result<(), Error> {
        self.calls
            .run_log(&self.work, level, message.into(), step_index)
            .await
    }

    pub fn plan_declared(
        &self,
        steps: Vec<String>,
        declaration_id: Option<Uuid>,
    ) -> Result<WriteEnd, Error> {
        self.queue
            .upgrade()
            .ok_or_else(ended)?
            .plan_declared(steps, declaration_id)
    }

    pub fn step_started(
        &self,
        index: u32,
        label: impl Into<String>,
        started_at: DateTime<Utc>,
    ) -> Result<WriteEnd, Error> {
        Ok(self
            .queue
            .upgrade()
            .ok_or_else(ended)?
            .step_started(index, label.into(), started_at))
    }
}

impl std::fmt::Debug for RunContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunContext")
    }
}
