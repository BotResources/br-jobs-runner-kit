use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_nats::jetstream::{self, stream};
use br_agentic_contract::ThreadRef;
use br_runner_threads::transport::NatsBus;
use br_runner_threads::{NewThread, ThreadsClient};
use contract_threads::STREAMING_STREAM;
use contract_threads::coords::event_coords;
use futures::StreamExt;
use serde_json::json;
use uuid::Uuid;

pub async fn streams(client: &async_nats::Client, with_streaming: bool) {
    let js = jetstream::new(client.clone());
    let mut wanted = vec![
        ("INTEGRATION_CMD", "integration.cmd.>"),
        ("INTEGRATION_EVT", "integration.evt.>"),
    ];
    if with_streaming {
        wanted.push((STREAMING_STREAM, "stream.threads.>"));
    }
    for (name, subject) in wanted {
        js.create_stream(stream::Config {
            name: name.to_owned(),
            subjects: vec![subject.to_owned()],
            ..Default::default()
        })
        .await
        .expect("stream");
    }
}

#[derive(Clone, Default)]
pub struct Script {
    pub refuse: Arc<Mutex<HashSet<Uuid>>>,
    pub status: Arc<Mutex<Option<&'static str>>>,
    pub garbage: Arc<Mutex<bool>>,
}

pub fn answer_subject(aggregate: &str, refused: bool) -> String {
    let aggregate = if aggregate == "thread" {
        "thread"
    } else {
        "message"
    };
    let fact = if refused { "refused" } else { "confirmed" };
    br_util_nats_fabric::event_subject(&event_coords(aggregate, fact))
}

fn answer_payload(
    id: Uuid,
    command_type: &str,
    aggregate: &str,
    refused: bool,
    script: &Script,
) -> serde_json::Value {
    let garbage = *script.garbage.lock().expect("lock");
    let mut payload = json!({
        "command_id": id,
        "command_type": command_type,
        "thread_id": if garbage && aggregate == "thread" { json!("nope") } else { json!(Uuid::now_v7()) },
    });
    if aggregate == "message" {
        payload["message_id"] = json!(Uuid::now_v7());
    }
    if refused {
        payload["reason_code"] = json!("KEY_REUSED");
        payload["params"] = json!({ "n": 1 });
    } else if aggregate == "message" {
        let status = if garbage {
            "NOT_A_STATUS"
        } else {
            script.status.lock().expect("lock").unwrap_or("STREAMING")
        };
        payload["status"] = json!(status);
        payload["deleted"] = json!(false);
    }
    payload
}

pub async fn responder(
    client: async_nats::Client,
    script: Script,
    batch: usize,
) -> tokio::task::JoinHandle<Vec<String>> {
    let mut commands = client
        .subscribe("integration.cmd.>")
        .await
        .expect("subscribe");
    client.flush().await.expect("flush");
    tokio::spawn(async move {
        let mut held = Vec::new();
        let mut subjects = Vec::new();
        while let Some(message) = commands.next().await {
            let envelope: serde_json::Value =
                serde_json::from_slice(&message.payload).expect("a command envelope");
            let id: Uuid = envelope["command_id"]
                .as_str()
                .and_then(|text| text.parse().ok())
                .expect("a command carries its id");
            let command_type = envelope["command_type"]
                .as_str()
                .expect("a command type")
                .to_owned();
            let parts: Vec<String> = message
                .subject
                .as_str()
                .split('.')
                .map(str::to_owned)
                .collect();
            subjects.push(message.subject.to_string());
            held.push((id, parts[3].clone(), command_type));
            if held.len() < batch {
                continue;
            }
            for (id, aggregate, command_type) in held.drain(..).rev() {
                let refused = script.refuse.lock().expect("lock").contains(&id);
                let payload = answer_payload(id, &command_type, &aggregate, refused, &script);
                let body = json!({ "payload": payload });
                client
                    .publish(
                        answer_subject(&aggregate, refused),
                        serde_json::to_vec(&body).expect("json").into(),
                    )
                    .await
                    .expect("publish");
            }
            client.flush().await.expect("flush");
        }
        subjects
    })
}

pub fn new_thread_for(thread: ThreadRef) -> NewThread {
    NewThread {
        command_id: Uuid::now_v7(),
        thread,
        owner_id: None,
        member_ids: Vec::new(),
        title: None,
        resources: BTreeMap::new(),
        origin: None,
        acting_user_id: None,
        wait: Some(Duration::from_secs(10)),
    }
}

pub async fn threads_client(client: &async_nats::Client) -> ThreadsClient {
    let bus = NatsBus::connect(client.clone()).await.expect("bound");
    ThreadsClient::new(
        super::actor(),
        Arc::new(bus),
        Arc::new(br_runner_threads::transport::MemoryGraphql::new()),
    )
}

pub async fn stored_commands(client: &async_nats::Client) -> Vec<serde_json::Value> {
    let stream = jetstream::new(client.clone())
        .get_stream("INTEGRATION_CMD")
        .await
        .expect("stream");
    let consumer = stream
        .create_consumer(jetstream::consumer::pull::Config {
            ack_policy: jetstream::consumer::AckPolicy::None,
            ..Default::default()
        })
        .await
        .expect("consumer");
    let mut batch = consumer
        .fetch()
        .max_messages(100)
        .expires(Duration::from_millis(500))
        .messages()
        .await
        .expect("batch");
    let mut commands = Vec::new();
    while let Some(message) = batch.next().await {
        let message = message.expect("message");
        commands.push(serde_json::from_slice(&message.message.payload).expect("envelope"));
    }
    commands
}
