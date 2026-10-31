use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use br_runner_connection::{Connection, Error};
use bytes::Bytes;
use futures::StreamExt;

use super::{Counting, account};

pub struct Server {
    child: Option<Child>,
    port: u16,
    store: Option<PathBuf>,
    config: Option<PathBuf>,
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

fn spawn(port: u16, store: Option<&PathBuf>, config: Option<&PathBuf>) -> Child {
    let binary = std::env::var("NATS_SERVER_BIN").unwrap_or_else(|_| "nats-server".to_owned());
    let mut command = Command::new(binary);
    command.args(["-a", "127.0.0.1", "-p", &port.to_string()]);
    if let Some(store) = store {
        command.arg("-js").arg("-sd").arg(store);
    }
    if let Some(config) = config {
        command.arg("-c").arg(config);
    }
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("nats-server starts")
}

impl Server {
    pub fn start() -> Self {
        let port = free_port();
        Self {
            child: Some(spawn(port, None, None)),
            port,
            store: None,
            config: None,
        }
    }

    pub fn start_with_jetstream() -> Self {
        let port = free_port();
        let store = std::env::temp_dir().join(format!(
            "br-runner-connection-js-{port}-{}",
            std::process::id()
        ));
        Self {
            child: Some(spawn(port, Some(&store), None)),
            port,
            store: Some(store),
            config: None,
        }
    }

    pub fn start_with_max_payload(bytes: usize) -> Self {
        let port = free_port();
        let config = std::env::temp_dir().join(format!(
            "br-runner-connection-conf-{port}-{}.conf",
            std::process::id()
        ));
        std::fs::write(&config, format!("max_payload: {bytes}\n")).expect("config written");
        Self {
            child: Some(spawn(port, None, Some(&config))),
            port,
            store: None,
            config: Some(config),
        }
    }

    pub fn url(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn restart(&mut self) {
        self.kill();
        self.child = Some(spawn(self.port, self.store.as_ref(), self.config.as_ref()));
    }

    pub async fn client(&self) -> async_nats::Client {
        for _ in 0..200 {
            if let Ok(client) = async_nats::connect(self.url()).await {
                return client;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("nats-server did not start");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill();
        if let Some(store) = &self.store {
            let _ = std::fs::remove_dir_all(store);
        }
        if let Some(config) = &self.config {
            let _ = std::fs::remove_file(config);
        }
    }
}

pub const WAIT: Duration = Duration::from_secs(10);

pub async fn connect_to(client: async_nats::Client) -> (Connection, Arc<Counting>) {
    let credential = Counting::giving_token();
    let connection = Connection::new(
        "http://127.0.0.1:1/graphql",
        credential.clone(),
        client,
        account(),
        WAIT,
    )
    .expect("builds");
    (connection, credential)
}

pub async fn echo(client: async_nats::Client, subject: &'static str) {
    let mut subscription = client.subscribe(subject).await.expect("subscribed");
    client.flush().await.expect("flushed");
    tokio::spawn(async move {
        while let Some(message) = subscription.next().await {
            if let Some(reply) = message.reply {
                let mut body = b"echo:".to_vec();
                body.extend_from_slice(&message.payload);
                let _ = client.publish(reply, body.into()).await;
            }
        }
    });
}

pub async fn request(
    connection: &Connection,
    subject: &str,
    body: Bytes,
    wait: Duration,
) -> Result<Bytes, Error> {
    tokio::time::timeout(
        Duration::from_secs(15),
        connection.bus_request(subject, body, wait),
    )
    .await
    .expect("the request returned")
}
