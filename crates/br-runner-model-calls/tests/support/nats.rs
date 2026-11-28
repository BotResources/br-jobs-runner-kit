//! A real nats-server with JetStream, started per test, with the stream of the
//! integration commands.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use async_nats::jetstream::{self, stream};
use br_util_nats_fabric::Fabric;
use serde_json::Value;

pub const STREAM: &str = "INTEGRATION_CMD";

pub struct Bus {
    child: Option<Child>,
    port: u16,
    store: PathBuf,
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

fn spawn(port: u16, store: &PathBuf) -> Child {
    let binary = std::env::var("NATS_SERVER_BIN").unwrap_or_else(|_| "nats-server".to_owned());
    Command::new(binary)
        .args(["-a", "127.0.0.1", "-p", &port.to_string(), "-js", "-sd"])
        .arg(store)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("nats-server starts")
}

/// One message of the stream.
pub struct Stored {
    pub message_id: Option<String>,
    pub subject: String,
    pub body: Value,
}

impl Bus {
    /// Starts the server and creates the command stream.
    pub async fn start() -> Self {
        let port = free_port();
        let store = std::env::temp_dir().join(format!(
            "br-runner-model-calls-js-{port}-{}",
            std::process::id()
        ));
        let bus = Self {
            child: Some(spawn(port, &store)),
            port,
            store,
        };
        let js = jetstream::new(bus.client().await);
        js.get_or_create_stream(stream::Config {
            name: STREAM.to_owned(),
            subjects: vec!["integration.cmd.>".to_owned()],
            duplicate_window: Duration::from_secs(120),
            ..Default::default()
        })
        .await
        .expect("the stream");
        bus
    }

    pub fn url(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    pub async fn client(&self) -> async_nats::Client {
        for _ in 0..400 {
            if let Ok(client) = async_nats::ConnectOptions::new()
                .retry_on_initial_connect()
                .connection_timeout(Duration::from_millis(200))
                .connect(self.url())
                .await
                && client.flush().await.is_ok()
            {
                return client;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("nats-server did not start");
    }

    pub async fn fabric(&self) -> Fabric {
        Fabric::new(jetstream::new(self.client().await))
    }

    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn restart(&mut self) {
        self.kill();
        self.child = Some(spawn(self.port, &self.store));
    }

    /// Every message of the command stream, in order.
    pub async fn messages(&self) -> Vec<Stored> {
        let js = jetstream::new(self.client().await);
        let stream = js.get_stream(STREAM).await.expect("the stream");
        let count = stream.get_info().await.expect("info").state.last_sequence;
        let mut out = Vec::new();
        for sequence in 1..=count {
            let Ok(message) = stream.get_raw_message(sequence).await else {
                continue;
            };
            out.push(Stored {
                message_id: message
                    .headers
                    .get(async_nats::header::NATS_MESSAGE_ID)
                    .map(|v| v.as_str().to_owned()),
                subject: message.subject.to_string(),
                body: serde_json::from_slice(&message.payload).expect("json"),
            });
        }
        out
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        self.kill();
        let _ = std::fs::remove_dir_all(&self.store);
    }
}

/// A client of a server that never answers: every publish waits.
pub async fn unreachable_fabric() -> Fabric {
    let client = async_nats::ConnectOptions::new()
        .retry_on_initial_connect()
        .connect("nats://127.0.0.1:1")
        .await
        .expect("a client that retries");
    Fabric::new(jetstream::new(client))
}
