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
    loop {
        let sent =
            tokio::time::timeout(shared.settings.outcome_wait, shared.sender.send(&write)).await;
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

fn unsent(
    shared: &Arc<Shared>,
    current: Option<Entry>,
    held: HashMap<u64, (Entry, Option<Write>)>,
) {
    let mut entries: Vec<Entry> = current.into_iter().collect();
    entries.extend(held.into_values().map(|(entry, _)| entry));
    {
        let mut state = shared.lock();
        state.stopped = true;
        entries.extend(state.fifo.drain(..));
        entries.extend(state.new_held.drain(..).map(|held| held.entry));
    }
    for entry in entries {
        shared.complete(entry, Err(Removal::LeftUnsent));
    }
}

pub(crate) async fn run(shared: Arc<Shared>) {
    let mut gates: FuturesUnordered<GateEnd> = FuturesUnordered::new();
    let mut held: HashMap<u64, (Entry, Option<Write>)> = HashMap::new();
    let mut entry: Option<Entry> = None;
    let mut attempt: Option<Attempt> = None;
    loop {
        {
            let mut state = shared.lock();
            for item in state.new_held.drain(..) {
                let seq = item.entry.seq;
                let mut gate = item.gate;
                gates.push(Box::pin(async move { (seq, (&mut gate).await) }));
                held.insert(seq, (item.entry, item.follow));
            }
        }
        if entry.is_none()
            && let Some((next, next_attempt)) = next_head(&shared)
        {
            entry = Some(next);
            attempt = Some(next_attempt);
        }
        let attempt_done = async {
            match attempt.as_mut() {
                Some(running) => running.as_mut().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            () = shared.stop.cancelled() => {
                drop(gates);
                unsent(&shared, entry.take(), held);
                return;
            }
            Some((seq, gate)) = gates.next(), if !gates.is_empty() => {
                if let Some((waiting, follow)) = held.remove(&seq) {
                    gate_ended(&shared, waiting, follow, gate);
                }
            }
            result = attempt_done => {
                attempt = None;
                if let Some(finished) = entry.take() {
                    shared.complete(finished, result);
                }
            }
            () = shared.wake.notified() => {}
        }
    }
}
