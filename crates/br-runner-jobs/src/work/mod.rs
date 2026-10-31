mod core;
mod parse;
mod sender;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use br_core_auth::Passport;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::backend::{Backend, ChildCommand, LogRecord, TriggerDelivery};
use crate::error::Error;
use crate::graphql;
use crate::model::{CallMode, Content, LogLevel, PayloadForm, Restart, ResultRef, SavedState};
use crate::queue::{DeliveryQueue, QueueSettings, WriteEnd};

pub(crate) use core::WorkCore;
pub use parse::Unreadable;
pub(crate) use parse::trigger as parse_trigger;
pub(crate) use sender::WorkSender;

pub struct Work {
    core: Arc<WorkCore>,
    attempt: u32,
    config: Option<Value>,
    passport: Passport,
    triggered_by: Option<Uuid>,
    payload: Option<PayloadForm>,
    call: Option<CallMode>,
    restart: Option<Restart>,
    unknown: Map<String, Value>,
    delivery: Option<Box<dyn TriggerDelivery>>,
}

impl fmt::Debug for Work {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Work")
            .field("attempt", &self.attempt)
            .field("call", &self.call)
            .field("restart", &self.restart)
            .field("from_trigger", &self.delivery.is_some())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for Work {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a Work")
    }
}

impl Work {
    pub(crate) fn from_value(
        backend: Arc<dyn Backend>,
        value: Value,
        delivery: Option<Box<dyn TriggerDelivery>>,
    ) -> Result<Self, Unreadable> {
        let parsed = parse_trigger(value)?;
        Ok(Self {
            core: Arc::new(WorkCore {
                backend,
                job_id: parsed.job_id,
                run_id: parsed.run_id,
                parent_job_id: parsed.parent_job_id,
                initial_job_id: parsed.initial_job_id,
                passport_json: parsed.passport_json,
                queue_made: AtomicBool::new(false),
                book_made: AtomicBool::new(false),
            }),
            attempt: parsed.attempt,
            config: parsed.config,
            passport: parsed.passport,
            triggered_by: parsed.triggered_by,
            payload: parsed.payload,
            call: parsed.call,
            restart: parsed.restart,
            unknown: parsed.unknown,
            delivery,
        })
    }

    pub(crate) fn with_delivery(mut self, delivery: Box<dyn TriggerDelivery>) -> Self {
        self.delivery = Some(delivery);
        self
    }

    pub(crate) fn core(&self) -> &Arc<WorkCore> {
        &self.core
    }

    pub fn job_id(&self) -> Uuid {
        self.core.job_id
    }

    pub fn run_id(&self) -> Uuid {
        self.core.run_id
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn config(&self) -> Option<&Value> {
        self.config.as_ref()
    }

    pub fn parent_job_id(&self) -> Uuid {
        self.core.parent_job_id
    }

    pub fn initial_job_id(&self) -> Uuid {
        self.core.initial_job_id
    }

    pub fn passport(&self) -> &Passport {
        &self.passport
    }

    pub fn triggered_by(&self) -> Option<Uuid> {
        self.triggered_by
    }

    pub fn payload_form(&self) -> Option<&PayloadForm> {
        self.payload.as_ref()
    }

    pub fn call(&self) -> Option<CallMode> {
        self.call
    }

    pub fn restart(&self) -> Option<Restart> {
        self.restart
    }

    pub fn unknown_fields(&self) -> &Map<String, Value> {
        &self.unknown
    }

    fn delivery(&self) -> Result<&dyn TriggerDelivery, Error> {
        self.delivery
            .as_deref()
            .ok_or_else(|| Error::invalid("this Work came from a claim, it has no trigger"))
    }

    pub async fn ack(&self) -> Result<(), Error> {
        self.delivery()?.ack().await
    }

    pub async fn give_back(self) -> Result<(), Error> {
        self.delivery()?.give_back().await
    }

    pub async fn terminate(self) -> Result<(), Error> {
        self.delivery()?.terminate().await
    }

    pub async fn payload(&self) -> Result<Content, Error> {
        graphql::payload(self.core.backend.as_ref(), &self.core.ids()).await
    }

    pub async fn saved_state(&self) -> Result<SavedState, Error> {
        graphql::saved_state(self.core.backend.as_ref(), &self.core.ids()).await
    }

    pub async fn read_result(&self, result: ResultRef, keep: bool) -> Result<Content, Error> {
        graphql::read_result(self.core.backend.as_ref(), &self.core.ids(), result, keep).await
    }

    pub async fn cancel_own_job(&self, command_id: Uuid) -> Result<(), Error> {
        let backend = &self.core.backend;
        backend
            .child_command(
                ChildCommand::Cancel {
                    command_id,
                    job_id: self.core.job_id,
                    run_id: self.core.run_id,
                },
                backend.reply_wait(),
            )
            .await
    }

    pub async fn log(
        &self,
        level: LogLevel,
        message: impl Into<String>,
        step_index: Option<u32>,
        id: Option<Uuid>,
    ) -> Result<(), Error> {
        self.core
            .backend
            .publish_log(LogRecord {
                id,
                run_id: self.core.run_id,
                step_index,
                level,
                message: message.into(),
                logged_at: chrono::Utc::now(),
            })
            .await
    }

    pub async fn delivery_queue(
        &self,
        settings: QueueSettings,
    ) -> Result<(DeliveryQueue, WriteEnd), Error> {
        if settings.outcome_wait.is_zero() || settings.retry_wait.is_zero() {
            return Err(Error::invalid("a wait of the delivery queue is zero"));
        }
        WorkCore::claim_once(&self.core.queue_made, "delivery queue")?;
        let sender = Arc::new(WorkSender {
            core: Arc::clone(&self.core),
            outcome_wait: settings.outcome_wait,
        });
        DeliveryQueue::start(sender, settings, self.core.run_id)
    }
}
