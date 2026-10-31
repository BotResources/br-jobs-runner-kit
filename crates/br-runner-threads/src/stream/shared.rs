use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use contract_threads::Chunk;
use uuid::Uuid;

use crate::backend::Backend;
use crate::error::Error;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Closing {
    Waiting,
    Refused,
    Lost,
    Confirmed,
}

#[derive(Default)]
struct Flags {
    broken: bool,
    closings: HashMap<u64, Closing>,
    next_closing: u64,
}

pub(crate) struct Core {
    pub(crate) backend: Arc<dyn Backend>,
    pub(crate) message_id: Uuid,
    pub(crate) wait: Duration,
    pub(crate) gate: tokio::sync::Mutex<Vec<Chunk>>,
    flags: Mutex<Flags>,
}

fn locked(flags: &Mutex<Flags>) -> MutexGuard<'_, Flags> {
    flags
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Core {
    pub(crate) fn new(backend: Arc<dyn Backend>, message_id: Uuid, wait: Duration) -> Arc<Self> {
        Arc::new(Self {
            backend,
            message_id,
            wait,
            gate: tokio::sync::Mutex::new(Vec::new()),
            flags: Mutex::new(Flags::default()),
        })
    }

    fn confirmed(flags: &Flags) -> bool {
        flags.closings.values().any(|c| *c == Closing::Confirmed)
    }

    pub(crate) fn judge_chunk(&self) -> Result<(), Error> {
        let flags = locked(&self.flags);
        if Self::confirmed(&flags) {
            return Err(Error::StreamClosed);
        }
        if flags.broken {
            return Err(Error::StreamBroken);
        }
        if flags.closings.values().any(|c| *c != Closing::Refused) {
            return Err(Error::StreamClosed);
        }
        Ok(())
    }

    pub(crate) fn judge_closing(&self, sealed: bool) -> Result<(), Error> {
        let flags = locked(&self.flags);
        if Self::confirmed(&flags) {
            return Err(Error::StreamClosed);
        }
        if sealed && flags.broken {
            return Err(Error::StreamBroken);
        }
        Ok(())
    }

    pub(crate) fn healthy(&self) -> bool {
        let flags = locked(&self.flags);
        !flags.broken && flags.closings.values().all(|c| *c == Closing::Refused)
    }

    pub(crate) async fn push(
        self: &Arc<Self>,
        build: impl FnOnce() -> Result<Chunk, Error>,
    ) -> Result<(), Error> {
        let mut chunks = self.gate.lock().await;
        self.judge_chunk()?;
        let chunk = build()?;
        let seq = u64::try_from(chunks.len()).map_err(|_| Error::StreamBroken)?;
        chunks.push(chunk.clone());
        let guard = BreakOnDrop::arm(self);
        let published = self
            .backend
            .chunk(self.message_id, seq, chunk, self.wait)
            .await;
        guard.disarm();
        match published {
            Ok(()) => Ok(()),
            Err(_) => {
                self.break_handle();
                Err(Error::StreamBroken)
            }
        }
    }

    fn break_handle(&self) {
        locked(&self.flags).broken = true;
    }

    pub(crate) fn begin_closing(self: &Arc<Self>) -> ClosingGuard {
        let mut flags = locked(&self.flags);
        let id = flags.next_closing;
        flags.next_closing += 1;
        flags.closings.insert(id, Closing::Waiting);
        ClosingGuard {
            core: Arc::clone(self),
            id,
            settled: false,
        }
    }

    fn settle(&self, id: u64, outcome: Option<Closing>) {
        let mut flags = locked(&self.flags);
        match outcome {
            Some(outcome) => {
                flags.closings.insert(id, outcome);
            }
            None => {
                flags.closings.remove(&id);
            }
        }
    }
}

struct BreakOnDrop<'a> {
    core: &'a Core,
    armed: bool,
}

impl<'a> BreakOnDrop<'a> {
    fn arm(core: &'a Core) -> Self {
        Self { core, armed: true }
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for BreakOnDrop<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.core.break_handle();
        }
    }
}

pub(crate) struct ClosingGuard {
    core: Arc<Core>,
    id: u64,
    settled: bool,
}

impl ClosingGuard {
    pub(crate) fn unsent(mut self) {
        self.settled = true;
        self.core.settle(self.id, None);
    }

    pub(crate) fn refused(mut self) {
        self.settled = true;
        self.core.settle(self.id, Some(Closing::Refused));
    }

    pub(crate) fn confirmed(mut self) {
        self.settled = true;
        self.core.settle(self.id, Some(Closing::Confirmed));
    }

    pub(crate) fn lost(mut self) {
        self.settled = true;
        self.core.settle(self.id, Some(Closing::Lost));
    }
}

impl Drop for ClosingGuard {
    fn drop(&mut self) {
        if !self.settled {
            self.core.settle(self.id, Some(Closing::Lost));
        }
    }
}
