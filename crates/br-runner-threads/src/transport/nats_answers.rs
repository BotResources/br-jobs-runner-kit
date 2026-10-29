use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use async_nats::Message;
use contract_threads::{
    MessageCommandConfirmed, MessageCommandRefused, ReasonCode, ThreadCommandConfirmed,
    ThreadCommandRefused, ThreadsEvent,
};
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

fn locked(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

type Reader = fn(Value) -> Result<Answer, Error>;

fn routes() -> Vec<(String, Reader)> {
    vec![
        (
            subject_of::<ThreadCommandConfirmed>(),
            thread_confirmed as Reader,
        ),
        (subject_of::<ThreadCommandRefused>(), thread_refused),
        (subject_of::<MessageCommandConfirmed>(), message_confirmed),
        (subject_of::<MessageCommandRefused>(), message_refused),
    ]
}

fn subject_of<E: ThreadsEvent>() -> String {
    br_util_nats_fabric::event_subject(&E::coords())
}

fn unreadable(error: serde_json::Error) -> Error {
    Error::Unreadable(format!("an answer cannot be read: {error}"))
}

fn status_of(status: contract_threads::MessageStatus) -> MessageStatus {
    match status {
        contract_threads::MessageStatus::Streaming => MessageStatus::Streaming,
        contract_threads::MessageStatus::Completed => MessageStatus::Completed,
        contract_threads::MessageStatus::Failed => MessageStatus::Failed,
        contract_threads::MessageStatus::Cancelled => MessageStatus::Cancelled,
    }
}

fn refusal(command_id: Uuid, code: ReasonCode, params: Map<String, Value>) -> Answer {
    Answer::Refused(Refusal {
        code: code.as_str().to_owned(),
        params,
        command_id: Some(command_id),
    })
}

fn thread_confirmed(payload: Value) -> Result<Answer, Error> {
    let event: ThreadCommandConfirmed = serde_json::from_value(payload).map_err(unreadable)?;
    Ok(Answer::Confirmed(Confirmed {
        command_id: event.command_id,
        status: None,
    }))
}

fn thread_refused(payload: Value) -> Result<Answer, Error> {
    let event: ThreadCommandRefused = serde_json::from_value(payload).map_err(unreadable)?;
    Ok(refusal(event.command_id, event.reason_code, event.params))
}

fn message_confirmed(payload: Value) -> Result<Answer, Error> {
    let event: MessageCommandConfirmed = serde_json::from_value(payload).map_err(unreadable)?;
    Ok(Answer::Confirmed(Confirmed {
        command_id: event.command_id,
        status: Some(status_of(event.status)),
    }))
}

fn message_refused(payload: Value) -> Result<Answer, Error> {
    let event: MessageCommandRefused = serde_json::from_value(payload).map_err(unreadable)?;
    Ok(refusal(event.command_id, event.reason_code, event.params))
}

fn command_id_of(payload: &Value) -> Option<Uuid> {
    payload
        .get("command_id")?
        .as_str()
        .and_then(|text| Uuid::try_parse(text).ok())
}

fn read_answer(subject: &str, bytes: &[u8]) -> Option<(Uuid, Result<Answer, Error>)> {
    let (_, read) = routes()
        .into_iter()
        .find(|(candidate, _)| candidate == subject)?;
    let envelope = serde_json::from_slice::<Envelope>(bytes).ok()?;
    let command_id = command_id_of(&envelope.payload)?;
    Some((command_id, read(envelope.payload)))
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
        for (subject, _) in routes() {
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
        let Some((command_id, answer)) = read_answer(message.subject.as_str(), &message.payload)
        else {
            return;
        };
        let waiters = locked(&self.state)
            .waiters
            .remove(&command_id)
            .unwrap_or_default();
        for (_, sender) in waiters {
            let _ = sender.send(answer.clone());
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

#[cfg(test)]
#[path = "nats_answers_tests.rs"]
mod tests;
