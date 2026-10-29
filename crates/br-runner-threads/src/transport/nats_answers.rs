use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use async_nats::Message;
use contract_threads::coords::event_coords;
use futures::StreamExt;
use futures::stream::select_all;
use serde::Deserialize;
use serde_json::{Map, Value};
use tokio::sync::oneshot;
use uuid::Uuid;

use super::{Answer, AnswerWait, Confirmed};
use crate::error::{Error, Refusal};
use crate::message::MessageStatus;

type Sender = oneshot::Sender<Result<Answer, Error>>;

#[derive(Default)]
struct State {
    open: bool,
    next_token: u64,
    waiters: HashMap<Uuid, Vec<(u64, Sender)>>,
}

pub(crate) struct Answers {
    client: async_nats::Client,
    opening: tokio::sync::Mutex<()>,
    state: Mutex<State>,
}

#[derive(Deserialize)]
struct Envelope {
    payload: Value,
}

#[derive(Deserialize)]
struct Probe {
    command_id: Uuid,
}

#[derive(Deserialize)]
struct ConfirmedWire {
    status: Option<String>,
}

#[derive(Deserialize)]
struct RefusedWire {
    reason_code: String,
    #[serde(default)]
    params: Map<String, Value>,
}

fn locked(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn subjects() -> Vec<String> {
    [
        ("thread", "confirmed"),
        ("thread", "refused"),
        ("message", "confirmed"),
        ("message", "refused"),
    ]
    .into_iter()
    .map(|(aggregate, fact)| br_util_nats_fabric::event_subject(&event_coords(aggregate, fact)))
    .collect()
}

fn status_of(literal: &str) -> Result<MessageStatus, Error> {
    match contract_threads::MessageStatus::parse(literal) {
        Ok(contract_threads::MessageStatus::Streaming) => Ok(MessageStatus::Streaming),
        Ok(contract_threads::MessageStatus::Completed) => Ok(MessageStatus::Completed),
        Ok(contract_threads::MessageStatus::Failed) => Ok(MessageStatus::Failed),
        Ok(contract_threads::MessageStatus::Cancelled) => Ok(MessageStatus::Cancelled),
        Err(_) => Err(Error::Unreadable(format!("unknown status {literal}"))),
    }
}

fn decode(subject: &str, command_id: Uuid, payload: Value) -> Result<Answer, Error> {
    if subject.ends_with(".refused.v1") {
        let wire: RefusedWire = serde_json::from_value(payload)
            .map_err(|e| Error::Unreadable(format!("a refusal cannot be read: {e}")))?;
        return Ok(Answer::Refused(Refusal {
            code: wire.reason_code,
            params: wire.params,
            command_id: Some(command_id),
        }));
    }
    let wire: ConfirmedWire = serde_json::from_value(payload)
        .map_err(|e| Error::Unreadable(format!("a confirmation cannot be read: {e}")))?;
    let status = wire.status.as_deref().map(status_of).transpose()?;
    Ok(Answer::Confirmed(Confirmed { command_id, status }))
}

impl Answers {
    pub(crate) fn new(client: async_nats::Client) -> Arc<Self> {
        Arc::new(Self {
            client,
            opening: tokio::sync::Mutex::new(()),
            state: Mutex::new(State::default()),
        })
    }

    pub(crate) async fn ensure_open(self: &Arc<Self>) -> Result<(), Error> {
        let _opening = self.opening.lock().await;
        if locked(&self.state).open {
            return Ok(());
        }
        let mut subscribers = Vec::new();
        for subject in subjects() {
            let subscriber = self
                .client
                .subscribe(subject)
                .await
                .map_err(|e| Error::Unavailable(e.to_string()))?;
            subscribers.push(subscriber);
        }
        self.client
            .flush()
            .await
            .map_err(|e| Error::Unavailable(e.to_string()))?;
        locked(&self.state).open = true;
        let answers = Arc::clone(self);
        tokio::spawn(async move {
            let mut messages = select_all(subscribers);
            while let Some(message) = messages.next().await {
                answers.dispatch(&message);
            }
            answers.closed();
        });
        Ok(())
    }

    fn dispatch(&self, message: &Message) {
        let Ok(envelope) = serde_json::from_slice::<Envelope>(&message.payload) else {
            return;
        };
        let Ok(probe) = serde_json::from_value::<Probe>(envelope.payload.clone()) else {
            return;
        };
        let waiters = locked(&self.state)
            .waiters
            .remove(&probe.command_id)
            .unwrap_or_default();
        for (_, sender) in waiters {
            let answer = decode(
                message.subject.as_str(),
                probe.command_id,
                envelope.payload.clone(),
            );
            let _ = sender.send(answer);
        }
    }

    fn closed(&self) {
        let waiters = {
            let mut state = locked(&self.state);
            state.open = false;
            std::mem::take(&mut state.waiters)
        };
        for (_, senders) in waiters {
            for (_, sender) in senders {
                let _ = sender.send(Err(Error::AnswerLost(
                    "the answer subscription closed".to_owned(),
                )));
            }
        }
    }

    pub(crate) fn register(self: &Arc<Self>, command_id: Uuid) -> Waiter {
        let (sender, receiver) = oneshot::channel();
        let mut state = locked(&self.state);
        let token = state.next_token;
        state.next_token += 1;
        state
            .waiters
            .entry(command_id)
            .or_default()
            .push((token, sender));
        Waiter {
            answers: Arc::clone(self),
            command_id,
            token,
            receiver: Some(receiver),
        }
    }

    fn forget(&self, command_id: Uuid, token: u64) {
        let mut state = locked(&self.state);
        if let Some(senders) = state.waiters.get_mut(&command_id) {
            senders.retain(|(candidate, _)| *candidate != token);
            if senders.is_empty() {
                state.waiters.remove(&command_id);
            }
        }
    }
}

pub(crate) struct Waiter {
    answers: Arc<Answers>,
    command_id: Uuid,
    token: u64,
    receiver: Option<oneshot::Receiver<Result<Answer, Error>>>,
}

impl Waiter {
    pub(crate) fn into_wait(mut self) -> AnswerWait {
        let receiver = self.receiver.take();
        Box::pin(async move {
            let _waiter = self;
            match receiver {
                Some(receiver) => receiver.await.unwrap_or_else(|_| {
                    Err(Error::AnswerLost("the waiter was dropped".to_owned()))
                }),
                None => Err(Error::AnswerLost("the waiter was dropped".to_owned())),
            }
        })
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        self.answers.forget(self.command_id, self.token);
    }
}
