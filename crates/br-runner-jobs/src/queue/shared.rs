use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use tokio::sync::{Notify, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::backend::BoxFuture;
use crate::error::Error;
use crate::queue::deps::{DepKey, Ended, blocked, own_key};
use crate::queue::report::{LastFailure, QueueReport, QueueSettings, QueueState};
use crate::queue::sender::WriteSender;
use crate::queue::write::{Removal, Write, WriteEnd, WriteName, WriteResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Stored,
    Dropped,
}

pub(crate) struct Entry {
    pub seq: u64,
    pub write: Write,
    pub end: oneshot::Sender<WriteResult>,
}

pub(crate) struct Held {
    pub entry: Entry,
    pub follow: Option<Write>,
    pub gate: BoxFuture<'static, Gate>,
}

pub(crate) struct Pending {
    pub name: WriteName,
    pub handed_over: Instant,
}

pub(crate) struct State {
    pub fifo: VecDeque<Entry>,
    pub new_held: Vec<Held>,
    pub pending: BTreeMap<u64, Pending>,
    pub ended: HashMap<DepKey, Ended>,
    pub next_seq: u64,
    pub stopped: bool,
    pub last_failure: Option<(u64, LastFailure)>,
}

pub(crate) struct Shared {
    state: Mutex<State>,
    pub wake: Notify,
    pub progress: watch::Sender<u64>,
    pub stop: CancellationToken,
    pub sender: Arc<dyn WriteSender>,
    pub settings: QueueSettings,
}

impl Shared {
    pub(crate) fn new(sender: Arc<dyn WriteSender>, settings: QueueSettings) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                fifo: VecDeque::new(),
                new_held: Vec::new(),
                pending: BTreeMap::new(),
                ended: HashMap::new(),
                next_seq: 0,
                stopped: false,
                last_failure: None,
            }),
            wake: Notify::new(),
            progress: watch::channel(0).0,
            stop: CancellationToken::new(),
            sender,
            settings,
        })
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn report(&self, report: QueueReport) {
        self.settings.observer.report(&report);
    }

    pub(crate) fn progressed(&self) {
        self.progress.send_modify(|count| *count += 1);
    }

    fn register(&self, state: &mut State, write: &Write) -> u64 {
        let seq = state.next_seq;
        state.next_seq += 1;
        state.pending.insert(
            seq,
            Pending {
                name: write.name(),
                handed_over: Instant::now(),
            },
        );
        seq
    }

    pub(crate) fn hand_over(&self, write: Write) -> WriteEnd {
        let (end, report) = self.enqueue(write);
        if let Some(report) = report {
            self.report(report);
        }
        end
    }

    pub(crate) fn enqueue(&self, write: Write) -> (WriteEnd, Option<QueueReport>) {
        let (end_tx, end) = WriteEnd::pair();
        let mut state = self.lock();
        if state.stopped {
            drop(state);
            return self.finish_unqueued(write, Removal::LeftUnsent);
        }
        if let Some(removal) = blocked(&write, &state.ended) {
            drop(state);
            return self.finish_unqueued(write, removal);
        }
        let seq = self.register(&mut state, &write);
        state.fifo.push_back(Entry {
            seq,
            write,
            end: end_tx,
        });
        drop(state);
        self.wake.notify_one();
        (end, None)
    }

    pub(crate) fn hold(
        &self,
        write: Write,
        follow: Option<Write>,
        gate: BoxFuture<'static, Gate>,
    ) -> WriteEnd {
        let (end_tx, end) = WriteEnd::pair();
        let mut state = self.lock();
        if state.stopped {
            drop(state);
            return self.finish_reported(write, Removal::LeftUnsent);
        }
        let seq = self.register(&mut state, &write);
        state.new_held.push(Held {
            entry: Entry {
                seq,
                write,
                end: end_tx,
            },
            follow,
            gate,
        });
        drop(state);
        self.wake.notify_one();
        end
    }

    fn finish_reported(&self, write: Write, removal: Removal) -> WriteEnd {
        let (end, report) = self.finish_unqueued(write, removal);
        if let Some(report) = report {
            self.report(report);
        }
        end
    }

    fn finish_unqueued(&self, write: Write, removal: Removal) -> (WriteEnd, Option<QueueReport>) {
        let name = write.name();
        {
            let mut state = self.lock();
            if let Some(key) = own_key(&write) {
                state.ended.insert(key, Ended::of(&Err(removal.clone())));
            }
        }
        let report = QueueReport::Removed {
            write: name,
            removal: removal.clone(),
        };
        (WriteEnd::ended(Err(removal)), Some(report))
    }

    pub(crate) fn complete(&self, entry: Entry, result: WriteResult) {
        let name = entry.write.name();
        self.report(match &result {
            Ok(()) => QueueReport::Delivered { write: name },
            Err(removal) => QueueReport::Removed {
                write: name,
                removal: removal.clone(),
            },
        });
        {
            let mut state = self.lock();
            state.pending.remove(&entry.seq);
            if let Some(key) = own_key(&entry.write) {
                state.ended.insert(key, Ended::of(&result));
            }
            if state
                .last_failure
                .as_ref()
                .is_some_and(|(seq, _)| *seq == entry.seq)
            {
                state.last_failure = None;
            }
        }
        let _ = entry.end.send(result);
        self.progressed();
    }

    pub(crate) fn record_failure(&self, seq: u64, name: WriteName, error: Error) {
        self.lock().last_failure = Some((
            seq,
            LastFailure {
                write: name.clone(),
                error: error.clone(),
            },
        ));
        self.report(QueueReport::Retrying { write: name, error });
    }

    pub(crate) fn snapshot(&self) -> QueueState {
        let state = self.lock();
        QueueState {
            waiting: state.pending.values().map(|p| p.name.clone()).collect(),
            oldest_handed_over: state.pending.values().map(|p| p.handed_over).min(),
            last_failure: state.last_failure.as_ref().map(|(_, f)| f.clone()),
        }
    }

    pub(crate) fn stop_now(&self) {
        self.lock().stopped = true;
        self.stop.cancel();
    }

    pub(crate) async fn drained(&self) -> QueueState {
        let mut progress = self.progress.subscribe();
        let upto = self.lock().next_seq;
        loop {
            {
                let state = self.lock();
                let waiting = state
                    .pending
                    .keys()
                    .next()
                    .is_some_and(|first| *first < upto);
                if !waiting {
                    drop(state);
                    return self.snapshot();
                }
            }
            if progress.changed().await.is_err() {
                return self.snapshot();
            }
        }
    }
}
