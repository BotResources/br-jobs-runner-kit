use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Mutex, MutexGuard, PoisonError};

use tokio::sync::watch;
use uuid::Uuid;

use crate::error::Error;
use crate::model::SyncEnd;

pub(crate) type Outcome = Result<SyncEnd, Error>;

#[derive(Default)]
struct Held {
    outcomes: HashMap<Uuid, Outcome>,
    kept: Option<Error>,
    last: Option<Error>,
    generation: u64,
}

pub(crate) struct EndStore {
    held: Mutex<Held>,
    changed: watch::Sender<u64>,
}

pub(crate) struct Opened {
    pub generation: u64,
    pub changes: watch::Receiver<u64>,
}

impl EndStore {
    pub(crate) fn new() -> Self {
        Self {
            held: Mutex::new(Held::default()),
            changed: watch::channel(0).0,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn signal(&self) {
        self.changed.send_modify(|count| *count += 1);
    }

    pub(crate) fn record(&self, child_job_id: Uuid, outcome: Outcome) {
        let stored = match self.lock().outcomes.entry(child_job_id) {
            Entry::Occupied(_) => false,
            Entry::Vacant(slot) => {
                slot.insert(outcome);
                true
            }
        };
        if stored {
            self.signal();
        }
    }

    pub(crate) fn infra_missing(&self, error: Error) {
        {
            let mut held = self.lock();
            held.generation += 1;
            held.kept = Some(error.clone());
            held.last = Some(error);
        }
        self.signal();
    }

    pub(crate) fn reopened(&self) {
        self.lock().kept = None;
    }

    pub(crate) fn kept(&self) -> Option<Error> {
        self.lock().kept.clone()
    }

    pub(crate) fn open(&self) -> Opened {
        let held = self.lock();
        Opened {
            generation: held.generation,
            changes: self.changed.subscribe(),
        }
    }

    pub(crate) fn decide(&self, child_job_id: Uuid, opened: u64) -> Option<Outcome> {
        let held = self.lock();
        if let Some(outcome) = held.outcomes.get(&child_job_id) {
            return Some(outcome.clone());
        }
        if let Some(error) = &held.kept {
            return Some(Err(error.clone()));
        }
        if held.generation != opened {
            return Some(Err(held.last.clone().unwrap_or_else(|| {
                Error::InfraMissing("the arrival feed ended".to_owned())
            })));
        }
        None
    }
}
