use std::collections::HashSet;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_nats::jetstream::{self, stream};
use contract_threads::STREAMING_STREAM;
use contract_threads::coords::event_coords;
use futures::StreamExt;
use serde_json::json;
use uuid::Uuid;

pub struct Server {
    child: Child,
    port: u16,
    dir: std::path::PathBuf,
}

impl Server {
    pub fn start() -> Self {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port();
        let dir = std::env::temp_dir().join(format!("br-runner-threads-nats-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("dir");
        let binary = std::env::var("NATS_SERVER_BIN").unwrap_or_else(|_| "nats-server".to_owned());
        let child = Command::new(binary)
            .args(["-js", "-a", "127.0.0.1", "-p", &port.to_string(), "-sd"])
            .arg(&dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("nats-server starts");
        Self { child, port, dir }
    }

    pub async fn client(&self) -> async_nats::Client {
        for _ in 0..100 {
            if let Ok(client) = async_nats::connect(format!("127.0.0.1:{}", self.port)).await {
                return client;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("nats-server did not start");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

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
            let id: Uuid = message
                .headers
                .as_ref()
                .and_then(|h| h.get(async_nats::header::NATS_MESSAGE_ID))
                .and_then(|v| v.as_str().parse().ok())
                .expect("a command carries its id as the message id");
            let parts: Vec<String> = message
                .subject
                .as_str()
                .split('.')
                .map(str::to_owned)
                .collect();
            subjects.push(message.subject.to_string());
            held.push((id, parts[3].clone()));
            if held.len() < batch {
                continue;
            }
            for (id, aggregate) in held.drain(..).rev() {
                let refused = script.refuse.lock().expect("lock").contains(&id);
                let payload = if refused {
                    json!({ "command_id": id, "reason_code": "THREAD_EXISTS", "params": { "n": 1 } })
                } else if *script.garbage.lock().expect("lock") {
                    json!({ "command_id": id, "status": "NOT_A_STATUS" })
                } else {
                    let status = *script.status.lock().expect("lock");
                    json!({ "command_id": id, "status": status })
                };
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
