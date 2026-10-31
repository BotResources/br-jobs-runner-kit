use async_nats::HeaderMap;
use async_nats::jetstream::{self, stream};
use contract_threads::STREAMING_STREAM;

pub const COMMANDS: &str = "INTEGRATION_CMD";
pub const EVENTS: &str = "INTEGRATION_EVT";
pub const CHUNKS: &str = STREAMING_STREAM;

fn subject_of(name: &str) -> &'static str {
    match name {
        COMMANDS => "integration.cmd.>",
        EVENTS => "integration.evt.>",
        _ => "stream.threads.>",
    }
}

pub async fn create_stream(client: &async_nats::Client, name: &str) {
    jetstream::new(client.clone())
        .create_stream(stream::Config {
            name: name.to_owned(),
            subjects: vec![subject_of(name).to_owned()],
            ..Default::default()
        })
        .await
        .expect("a stream as gitops declares it");
}

pub async fn create_all_streams(client: &async_nats::Client) {
    for name in [COMMANDS, EVENTS, CHUNKS] {
        create_stream(client, name).await;
    }
}

pub async fn create_streams_except(client: &async_nats::Client, missing: &str) {
    for name in [COMMANDS, EVENTS, CHUNKS] {
        if name != missing {
            create_stream(client, name).await;
        }
    }
}

pub async fn delete_stream(client: &async_nats::Client, name: &str) {
    jetstream::new(client.clone())
        .delete_stream(name)
        .await
        .expect("the stream is deleted");
}

pub async fn stream_exists(client: &async_nats::Client, name: &str) -> bool {
    jetstream::new(client.clone())
        .get_stream(name)
        .await
        .is_ok()
}

pub async fn stored_count(client: &async_nats::Client, name: &str) -> u64 {
    match jetstream::new(client.clone()).get_stream(name).await {
        Ok(stream) => stream.get_info().await.expect("info").state.messages,
        Err(_) => 0,
    }
}

pub struct Stored {
    pub subject: String,
    pub headers: HeaderMap,
    pub payload: Vec<u8>,
}

impl Stored {
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.payload).expect("a stored payload that reads")
    }
}

pub async fn stored(client: &async_nats::Client, name: &str) -> Vec<Stored> {
    let stream = jetstream::new(client.clone())
        .get_stream(name)
        .await
        .expect("the stream");
    let count = stream.get_info().await.expect("info").state.messages;
    let mut messages = Vec::new();
    for sequence in 1..=count {
        let raw = stream.get_raw_message(sequence).await.expect("a message");
        messages.push(Stored {
            subject: raw.subject.to_string(),
            headers: raw.headers,
            payload: raw.payload.to_vec(),
        });
    }
    messages
}

pub async fn idle_bus() -> async_nats::Client {
    async_nats::ConnectOptions::new()
        .retry_on_initial_connect()
        .connect("nats://127.0.0.1:1")
        .await
        .expect("a client that retries")
}
