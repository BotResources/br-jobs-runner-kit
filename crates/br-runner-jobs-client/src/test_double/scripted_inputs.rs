use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use br_runner_connection::Connection;
use tokio::sync::{Notify, mpsc};

use super::scripted::{Channel, ScriptedJobs};
use crate::backend::{BoxFuture, IncomingTrigger, RawFeed, TriggerDelivery, TriggerSource};
use crate::error::Error;
use crate::model::{CancelEvent, ModeEvent, PresenceStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Ack,
    GiveBack,
    Terminate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceRecord {
    pub runner_version: String,
    pub status: PresenceStatus,
    pub capacity: u32,
}

type Failing = Mutex<Option<Error>>;

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

struct Triggers {
    queue: Mutex<VecDeque<Result<IncomingTrigger, Error>>>,
    arrived: Notify,
}

struct Source(Arc<Triggers>);

impl TriggerSource for Source {
    fn next(&mut self) -> BoxFuture<'_, Result<IncomingTrigger, Error>> {
        Box::pin(async move {
            loop {
                let arrived = self.0.arrived.notified();
                tokio::pin!(arrived);
                arrived.as_mut().enable();
                if let Some(next) = locked(&self.0.queue).pop_front() {
                    return next;
                }
                arrived.await;
            }
        })
    }
}

struct Recorded {
    log: Arc<Mutex<Vec<Delivery>>>,
    failing: Arc<Failing>,
}

impl Recorded {
    fn done(&self, delivery: Delivery) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            locked(&self.log).push(delivery);
            match locked(&self.failing).take() {
                Some(error) => Err(error),
                None => Ok(()),
            }
        })
    }
}

impl TriggerDelivery for Recorded {
    fn ack(&self) -> BoxFuture<'_, Result<(), Error>> {
        self.done(Delivery::Ack)
    }

    fn give_back(&self) -> BoxFuture<'_, Result<(), Error>> {
        self.done(Delivery::GiveBack)
    }

    fn terminate(&self) -> BoxFuture<'_, Result<(), Error>> {
        self.done(Delivery::Terminate)
    }
}

struct Slot<T> {
    sender: mpsc::UnboundedSender<Result<T, Error>>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<Result<T, Error>>>>,
}

impl<T: Send + 'static> Slot<T> {
    fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            sender,
            receiver: Mutex::new(Some(receiver)),
        }
    }

    fn open(&self) -> Result<Box<dyn RawFeed<T>>, Error> {
        let receiver = locked(&self.receiver)
            .take()
            .ok_or_else(|| Error::invalid("the scripted feed is open already"))?;
        Ok(Box::new(Channel(receiver)))
    }
}

pub(super) struct Inputs {
    claims: Mutex<VecDeque<Result<Vec<u8>, Error>>>,
    triggers: Arc<Triggers>,
    deliveries: Arc<Mutex<Vec<Delivery>>>,
    delivery_failing: Arc<Failing>,
    presence: Mutex<Vec<PresenceRecord>>,
    presence_failing: Failing,
    deleted: Mutex<u32>,
    log_failing: Failing,
    cancels: Slot<CancelEvent>,
    modes: Slot<ModeEvent>,
    ttl: Mutex<Result<Duration, Error>>,
    routed: Mutex<Option<Connection>>,
}

impl Inputs {
    pub(super) fn new() -> Self {
        Self {
            claims: Mutex::new(VecDeque::new()),
            triggers: Arc::new(Triggers {
                queue: Mutex::new(VecDeque::new()),
                arrived: Notify::new(),
            }),
            deliveries: Arc::new(Mutex::new(Vec::new())),
            delivery_failing: Arc::new(Mutex::new(None)),
            presence: Mutex::new(Vec::new()),
            presence_failing: Mutex::new(None),
            deleted: Mutex::new(0),
            log_failing: Mutex::new(None),
            cancels: Slot::new(),
            modes: Slot::new(),
            ttl: Mutex::new(Ok(Duration::from_secs(90))),
            routed: Mutex::new(None),
        }
    }

    pub(super) fn next_claim(&self) -> Result<Vec<u8>, Error> {
        locked(&self.claims)
            .pop_front()
            .unwrap_or_else(|| Ok(br"{}".to_vec()))
    }

    pub(super) fn trigger_source(&self) -> Box<dyn TriggerSource> {
        Box::new(Source(Arc::clone(&self.triggers)))
    }

    pub(super) fn put_presence(&self, record: PresenceRecord) -> Result<(), Error> {
        locked(&self.presence).push(record);
        fail_once(&self.presence_failing)
    }

    pub(super) fn delete_presence(&self) -> Result<(), Error> {
        *locked(&self.deleted) += 1;
        fail_once(&self.presence_failing)
    }

    pub(super) fn log_result(&self) -> Result<(), Error> {
        fail_once(&self.log_failing)
    }

    pub(super) fn routed(&self) -> Option<Connection> {
        locked(&self.routed).clone()
    }

    pub(super) fn ttl(&self) -> Result<Duration, Error> {
        locked(&self.ttl).clone()
    }

    pub(super) fn cancel_feed(&self) -> Result<Box<dyn RawFeed<CancelEvent>>, Error> {
        self.cancels.open()
    }

    pub(super) fn mode_feed(&self) -> Result<Box<dyn RawFeed<ModeEvent>>, Error> {
        self.modes.open()
    }
}

fn fail_once(slot: &Failing) -> Result<(), Error> {
    match locked(slot).take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

impl ScriptedJobs {
    pub fn push_claim_reply(&self, reply: Result<Vec<u8>, Error>) {
        locked(&self.inputs.claims).push_back(reply);
    }

    pub fn push_trigger(&self, raw: impl Into<Vec<u8>>) {
        let delivery = Box::new(Recorded {
            log: Arc::clone(&self.inputs.deliveries),
            failing: Arc::clone(&self.inputs.delivery_failing),
        });
        locked(&self.inputs.triggers.queue).push_back(Ok(IncomingTrigger {
            raw: raw.into(),
            delivery,
        }));
        self.inputs.triggers.arrived.notify_waiters();
    }

    pub fn push_trigger_error(&self, error: Error) {
        locked(&self.inputs.triggers.queue).push_back(Err(error));
        self.inputs.triggers.arrived.notify_waiters();
    }

    pub fn triggers_waiting(&self) -> usize {
        locked(&self.inputs.triggers.queue).len()
    }

    pub fn deliveries(&self) -> Vec<Delivery> {
        locked(&self.inputs.deliveries).clone()
    }

    pub fn fail_next_delivery(&self, error: Error) {
        *locked(&self.inputs.delivery_failing) = Some(error);
    }

    pub fn presence_records(&self) -> Vec<PresenceRecord> {
        locked(&self.inputs.presence).clone()
    }

    pub fn presence_deletions(&self) -> u32 {
        *locked(&self.inputs.deleted)
    }

    pub fn fail_next_presence(&self, error: Error) {
        *locked(&self.inputs.presence_failing) = Some(error);
    }

    pub fn fail_next_log(&self, error: Error) {
        *locked(&self.inputs.log_failing) = Some(error);
    }

    pub fn set_presence_ttl(&self, ttl: Result<Duration, Error>) {
        *locked(&self.inputs.ttl) = ttl;
    }

    pub fn route_graphql_through(&self, connection: Connection) {
        *locked(&self.inputs.routed) = Some(connection);
    }

    pub fn push_cancel(&self, event: Result<CancelEvent, Error>) {
        let _ = self.inputs.cancels.sender.send(event);
    }

    pub fn push_mode(&self, event: Result<ModeEvent, Error>) {
        let _ = self.inputs.modes.sender.send(event);
    }
}
