use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures::StreamExt;
use futures::stream::FuturesUnordered;

use crate::error::{CredentialKind, Error};
use crate::queue::deps::blocked;
use crate::queue::shared::{Entry, Gate, Pending, Shared};
use crate::queue::write::{Removal, Write, WriteEnd, WriteResult};

type Attempt = Pin<Box<dyn Future<Output = WriteResult> + Send>>;
type GateEnd = Pin<Box<dyn Future<Output = (u64, Gate)> + Send>>;

pub(crate) fn resendable(write: &Write, error: &Error) -> bool {
    match error {
        Error::Unavailable(_) | Error::Lost(_) | Error::Unreadable(_) => true,
        Error::Credential(failure) => failure.kind == CredentialKind::FetchFailed,
        Error::Refused(refusal) => {
            matches!(write, Write::Started { .. }) && refusal.reason == "instance_unknown"
        }
        _ => false,
    }
}

async fn attempts(shared: Arc<Shared>, seq: u64, write: Write) -> WriteResult {
    let name = write.name();
    let mut session = shared.sender.session(&write);
    loop {
        let sent = tokio::time::timeout(shared.settings.outcome_wait, session.send()).await;
        let error = match sent {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(error)) => error,
            Err(_) => Error::wait_fired("no outcome within the outcome wait"),
        };
        if resendable(&write, &error) {
            shared.record_failure(seq, name.clone(), error);
            tokio::time::sleep(shared.settings.retry_wait).await;
            continue;
        }
        return Err(match error {
            Error::Refused(_) => Removal::Refused(error),
            other => Removal::Failed(other),
        });
    }
}

fn next_head(shared: &Arc<Shared>) -> Option<(Entry, Attempt)> {
    loop {
        let mut state = shared.lock();
        if state.stopped {
            return None;
        }
        let head = state.fifo.pop_front()?;
        if let Some(removal) = blocked(&head.write, &state.ended) {
            drop(state);
            shared.complete(head, Err(removal));
            continue;
        }
        drop(state);
        let attempt: Attempt = Box::pin(attempts(Arc::clone(shared), head.seq, head.write.clone()));
        return Some((head, attempt));
    }
}

fn gate_ended(shared: &Arc<Shared>, entry: Entry, follow: Option<Write>, gate: Gate) {
    match gate {
        Gate::Stored => {
            let mut state = shared.lock();
            if state.stopped {
                drop(state);
                shared.complete(entry, Err(Removal::LeftUnsent));
                return;
            }
            if let Some(follow) = follow {
                let seq = state.next_seq;
                state.next_seq += 1;
                state.pending.insert(
                    seq,
                    Pending {
                        name: follow.name(),
                        handed_over: std::time::Instant::now(),
                    },
                );
                let (end, _) = WriteEnd::pair();
                state.fifo.push_back(Entry {
                    seq,
                    write: follow,
                    end,
                });
            }
            state.fifo.push_back(entry);
            drop(state);
            shared.wake.notify_one();
        }
        Gate::Dropped => shared.complete(entry, Err(Removal::DependsOnRemoved)),
    }
}

struct Task {
    shared: Arc<Shared>,
    gates: FuturesUnordered<GateEnd>,
    held: HashMap<u64, (Entry, Option<Write>)>,
    entry: Option<Entry>,
    attempt: Option<Attempt>,
}

impl Task {
    fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            gates: FuturesUnordered::new(),
            held: HashMap::new(),
            entry: None,
            attempt: None,
        }
    }

    fn take_new_held(&mut self) {
        let mut state = self.shared.lock();
        for item in state.new_held.drain(..) {
            let seq = item.entry.seq;
            let mut gate = item.gate;
            self.gates
                .push(Box::pin(async move { (seq, (&mut gate).await) }));
            self.held.insert(seq, (item.entry, item.follow));
        }
    }

    fn take_head(&mut self) {
        if self.entry.is_none()
            && let Some((next, next_attempt)) = next_head(&self.shared)
        {
            self.entry = Some(next);
            self.attempt = Some(next_attempt);
        }
    }

    async fn run(&mut self) {
        loop {
            self.take_new_held();
            self.take_head();
            let shared = Arc::clone(&self.shared);
            let attempt = &mut self.attempt;
            let attempt_done = async {
                match attempt.as_mut() {
                    Some(running) => running.as_mut().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                biased;
                () = shared.stop.cancelled() => return,
                Some((seq, gate)) = self.gates.next(), if !self.gates.is_empty() => {
                    if let Some((waiting, follow)) = self.held.remove(&seq) {
                        gate_ended(&shared, waiting, follow, gate);
                    }
                }
                result = attempt_done => {
                    self.attempt = None;
                    if let Some(finished) = self.entry.take() {
                        shared.complete(finished, result);
                    }
                }
                () = shared.wake.notified() => {}
            }
        }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        self.gates.clear();
        self.attempt = None;
        let mut entries: Vec<Entry> = self.entry.take().into_iter().collect();
        entries.extend(self.held.drain().map(|(_, (entry, _))| entry));
        {
            let mut state = self.shared.lock();
            state.stopped = true;
            entries.extend(state.fifo.drain(..));
            entries.extend(state.new_held.drain(..).map(|held| held.entry));
        }
        entries.sort_by_key(|entry| entry.seq);
        for entry in entries {
            self.shared.complete(entry, Err(Removal::LeftUnsent));
        }
    }
}

pub(crate) fn run(shared: Arc<Shared>) -> impl Future<Output = ()> + Send + 'static {
    let mut task = Task::new(shared);
    async move { task.run().await }
}
