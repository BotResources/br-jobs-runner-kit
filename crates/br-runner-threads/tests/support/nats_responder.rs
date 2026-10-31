use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use br_core_events::EventMetadata;
use br_core_integration::{IntegrationCommand, IntegrationEvent};
use br_util_nats_fabric::event_subject;
use contract_threads::{
    AiDelivery, FailMessageV2, MessageCommandConfirmed, MessageCommandRefused, MessageStatus,
    MessageTurn, ReasonCode, StoreMessageV2, ThreadCommandConfirmed, ThreadCommandRefused,
    ThreadsEvent,
};
use futures::StreamExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use uuid::Uuid;

use super::client_closing_x::kind_of;
use super::nats_streams::{CHUNKS, stored_count};
use br_runner_threads::ClosureKind;

#[derive(Clone)]
pub enum Reply {
    Confirm,
    Refuse(ReasonCode, Map<String, Value>),
    Unreadable,
    UnknownCode,
    Silence,
    StrayThenConfirm,
}

#[derive(Clone)]
pub struct Received {
    pub subject: String,
    pub envelope: IntegrationCommand<Value>,
    pub chunks_on_arrival: u64,
}

impl Received {
    pub fn id(&self) -> Uuid {
        self.envelope.command_id
    }

    pub fn command_type(&self) -> &str {
        &self.envelope.command_type
    }

    pub fn payload<T: DeserializeOwned>(&self) -> T {
        serde_json::from_value(self.envelope.payload.clone()).expect("the payload reads")
    }
}

type Scripts = Arc<Mutex<HashMap<Uuid, VecDeque<Reply>>>>;
type Arrivals = Arc<Mutex<VecDeque<Reply>>>;

pub struct Responder {
    received: Arc<Mutex<Vec<Received>>>,
    scripts: Scripts,
    arrivals: Arrivals,
    task: tokio::task::JoinHandle<()>,
}

impl Responder {
    pub async fn start(client: &async_nats::Client) -> Self {
        Self::batching(client, 1).await
    }

    pub async fn batching(client: &async_nats::Client, batch: usize) -> Self {
        let mut commands = client
            .subscribe("integration.cmd.>")
            .await
            .expect("the subscription");
        client.flush().await.expect("flushed");
        let received: Arc<Mutex<Vec<Received>>> = Arc::default();
        let scripts: Scripts = Arc::default();
        let arrivals: Arrivals = Arc::default();
        let task = tokio::spawn({
            let (client, received) = (client.clone(), received.clone());
            let (scripts, arrivals) = (scripts.clone(), arrivals.clone());
            async move {
                let mut held = Vec::new();
                while let Some(message) = commands.next().await {
                    let envelope = serde_json::from_slice(&message.payload).expect("an envelope");
                    let arrived = Received {
                        subject: message.subject.to_string(),
                        envelope,
                        chunks_on_arrival: stored_count(&client, CHUNKS).await,
                    };
                    received.lock().expect("lock").push(arrived.clone());
                    let reply = next_reply(&scripts, &arrivals, arrived.id());
                    held.push((arrived, reply));
                    if held.len() < batch {
                        continue;
                    }
                    for (arrived, reply) in held.drain(..).rev() {
                        answer(&client, &arrived, reply).await;
                    }
                }
            }
        });
        Self {
            received,
            scripts,
            arrivals,
            task,
        }
    }

    pub fn script(&self, command_id: Uuid, replies: impl IntoIterator<Item = Reply>) {
        self.scripts
            .lock()
            .expect("lock")
            .insert(command_id, replies.into_iter().collect());
    }

    pub fn script_arrivals(&self, replies: impl IntoIterator<Item = Reply>) {
        self.arrivals.lock().expect("lock").extend(replies);
    }

    pub fn received(&self) -> Vec<Received> {
        self.received.lock().expect("lock").clone()
    }

    pub fn received_of(&self, command_id: Uuid) -> Vec<Received> {
        self.received()
            .into_iter()
            .filter(|arrived| arrived.id() == command_id)
            .collect()
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn next_reply(scripts: &Scripts, arrivals: &Arrivals, command_id: Uuid) -> Reply {
    let scripted = scripts
        .lock()
        .expect("lock")
        .get_mut(&command_id)
        .and_then(VecDeque::pop_front);
    scripted
        .or_else(|| arrivals.lock().expect("lock").pop_front())
        .unwrap_or(Reply::Confirm)
}

fn uuid_of(payload: &Value, key: &str) -> Uuid {
    payload[key]
        .as_str()
        .and_then(|text| text.parse().ok())
        .expect("an id in the command")
}

fn failed_status(arrived: &Received) -> MessageStatus {
    let fail = arrived.payload::<FailMessageV2>();
    let kind = fail
        .closure
        .and_then(|closure| closure.to_closure_reason().ok())
        .map(|reason| kind_of(&reason));
    match kind {
        Some(ClosureKind::Cancelled) => MessageStatus::Cancelled,
        Some(ClosureKind::BudgetExhausted) => MessageStatus::Completed,
        _ => MessageStatus::Failed,
    }
}

fn status_of(arrived: &Received) -> MessageStatus {
    match arrived.command_type() {
        "message.complete" => MessageStatus::Completed,
        "message.fail" => failed_status(arrived),
        _ => match arrived.payload::<StoreMessageV2>().message {
            MessageTurn::Ai(turn) if matches!(turn.delivery, AiDelivery::Streaming) => {
                MessageStatus::Streaming
            }
            _ => MessageStatus::Completed,
        },
    }
}

fn outcome(arrived: &Received, reply: &Reply) -> (String, String, Value) {
    let payload = &arrived.envelope.payload;
    let (command_id, command_type) = (arrived.id(), arrived.command_type().to_owned());
    let thread_id = uuid_of(payload, "thread_id");
    let is_thread = command_type.starts_with("thread.");
    let refusal = match reply {
        Reply::Refuse(code, params) => Some((*code, params.clone())),
        Reply::UnknownCode => Some((ReasonCode::KeyReused, Map::new())),
        _ => None,
    };
    let (subject, event_type, mut body) = match (is_thread, refusal) {
        (true, None) => encoded(&ThreadCommandConfirmed {
            command_id,
            command_type,
            thread_id,
        }),
        (true, Some((reason_code, params))) => encoded(&ThreadCommandRefused {
            command_id,
            command_type,
            thread_id,
            reason_code,
            params,
        }),
        (false, None) => encoded(&MessageCommandConfirmed {
            command_id,
            command_type,
            thread_id,
            message_id: uuid_of(payload, "message_id"),
            status: status_of(arrived),
            deleted: false,
        }),
        (false, Some((reason_code, params))) => encoded(&MessageCommandRefused {
            command_id,
            command_type,
            thread_id,
            message_id: uuid_of(payload, "message_id"),
            reason_code,
            params,
        }),
    };
    if matches!(reply, Reply::Unreadable) {
        let key = if is_thread { "thread_id" } else { "status" };
        body[key] = Value::String("NOT_VALID".to_owned());
    }
    if matches!(reply, Reply::UnknownCode) {
        body["reason_code"] = Value::String("NOT_A_CODE".to_owned());
    }
    (subject, event_type, body)
}

fn encoded<T: ThreadsEvent + Serialize>(body: &T) -> (String, String, Value) {
    (
        event_subject(&T::coords()),
        T::event_type(),
        serde_json::to_value(body).expect("an answer serializes"),
    )
}

async fn publish(client: &async_nats::Client, arrived: &Received, reply: &Reply, to: Uuid) {
    let (subject, event_type, body) = outcome(arrived, reply);
    let event = IntegrationEvent::new(
        Uuid::now_v7(),
        event_type,
        1,
        chrono::Utc::now(),
        EventMetadata::new(arrived.envelope.metadata.actor, to),
        body,
    );
    client
        .publish(subject, serde_json::to_vec(&event).expect("json").into())
        .await
        .expect("published");
    client.flush().await.expect("flushed");
}

async fn answer(client: &async_nats::Client, arrived: &Received, reply: Reply) {
    match reply {
        Reply::Silence => {}
        Reply::StrayThenConfirm => {
            publish(client, arrived, &Reply::Confirm, Uuid::now_v7()).await;
            publish(client, arrived, &Reply::Confirm, arrived.id()).await;
        }
        other => publish(client, arrived, &other, arrived.id()).await,
    }
}
