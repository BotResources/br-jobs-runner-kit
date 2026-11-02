use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::{self, Context, kv, stream};
use br_core_kernel::ServiceAccountId;
use br_runner_connection::{Connection, FixedToken};
use br_util_nats_fabric::{
    INTEGRATION_CMD, INTEGRATION_EVT, KV_PUBLISHED_LANGUAGE, command_subject, event_subject,
};
use contract_jobs::runner::{
    CANCEL_BUCKET, LOG_FILTER, LOG_STREAM, PRESENCE_BUCKET, STATUS_FILTER, STATUS_STREAM,
    TRIGGER_FILTER, TRIGGER_STREAM,
};
use uuid::Uuid;

use super::server::Server;

pub const WAIT: Duration = Duration::from_secs(5);
pub const PRESENCE_TTL: Duration = Duration::from_secs(90);
pub const OFFLINE_ENDPOINT: &str = "http://127.0.0.1:9/graphql";

pub struct World {
    pub server: Server,
    pub client: async_nats::Client,
    pub js: Context,
}

fn wildcard(subject: &str) -> String {
    let head: Vec<&str> = subject.split('.').take(2).collect();
    format!("{}.>", head.join("."))
}

pub fn account() -> ServiceAccountId {
    ServiceAccountId(Uuid::from_u128(0xA0))
}

pub fn connection_on(client: &async_nats::Client, reply_wait: Duration) -> Connection {
    Connection::new(
        OFFLINE_ENDPOINT,
        Arc::new(FixedToken::new("a-token-for-the-stand-in")),
        client.clone(),
        account(),
        reply_wait,
    )
    .expect("a connection")
}

impl World {
    pub async fn bare() -> Self {
        let server = Server::start();
        let client = server.client().await;
        let js = jetstream::new(client.clone());
        Self { server, client, js }
    }

    pub async fn ready() -> Self {
        let world = Self::bare().await;
        world.create_runner_streams().await;
        world.create_runner_buckets(Some(PRESENCE_TTL)).await;
        world.create_platform().await;
        world
    }

    pub fn connection(&self) -> Connection {
        connection_on(&self.client, WAIT)
    }

    pub async fn create_stream(&self, name: &str, subject: String, workqueue: bool) {
        self.js
            .create_stream(stream::Config {
                name: name.to_owned(),
                subjects: vec![subject],
                retention: if workqueue {
                    stream::RetentionPolicy::WorkQueue
                } else {
                    stream::RetentionPolicy::Limits
                },
                ..Default::default()
            })
            .await
            .expect("a stream as gitops declares it");
    }

    pub async fn create_runner_streams(&self) {
        self.create_stream(TRIGGER_STREAM, TRIGGER_FILTER.to_owned(), true)
            .await;
        self.create_stream(STATUS_STREAM, STATUS_FILTER.to_owned(), false)
            .await;
        self.create_stream(LOG_STREAM, LOG_FILTER.to_owned(), false)
            .await;
    }

    pub async fn create_bucket(&self, name: &str, max_age: Option<Duration>) {
        self.js
            .create_key_value(kv::Config {
                bucket: name.to_owned(),
                history: 1,
                max_age: max_age.unwrap_or_default(),
                ..Default::default()
            })
            .await
            .expect("a bucket as gitops declares it");
    }

    pub async fn create_runner_buckets(&self, presence_ttl: Option<Duration>) {
        self.create_bucket(CANCEL_BUCKET, None).await;
        self.create_bucket(contract_jobs::runner::PRESENCE_BUCKET, presence_ttl)
            .await;
    }

    pub async fn create_platform(&self) {
        self.create_bucket(KV_PUBLISHED_LANGUAGE, None).await;
        let arrived = contract_jobs::evt_run_item_arrived_v2_coords().expect("coords");
        let created = contract_jobs::cmd_job_create_v3_coords().expect("coords");
        self.create_stream(INTEGRATION_EVT, wildcard(&event_subject(&arrived)), false)
            .await;
        self.create_stream(INTEGRATION_CMD, wildcard(&command_subject(&created)), false)
            .await;
    }

    pub async fn bucket(&self, name: &str) -> kv::Store {
        self.js.get_key_value(name).await.expect("the bucket")
    }

    pub async fn presence_bucket(&self) -> kv::Store {
        self.bucket(PRESENCE_BUCKET).await
    }

    pub async fn stored(&self, stream_name: &str) -> Vec<(String, Vec<u8>)> {
        let stream = self.js.get_stream(stream_name).await.expect("the stream");
        let last = stream.cached_info().state.last_sequence;
        let mut stream = stream;
        let last = stream
            .info()
            .await
            .map_or(last, |info| info.state.last_sequence);
        let mut found = Vec::new();
        for sequence in 1..=last {
            if let Ok(raw) = stream.get_raw_message(sequence).await {
                found.push((raw.subject.to_string(), raw.payload.to_vec()));
            }
        }
        found
    }

    pub async fn messages_in(&self, stream_name: &str) -> u64 {
        let mut stream = self.js.get_stream(stream_name).await.expect("the stream");
        stream.info().await.expect("info").state.messages
    }
}

pub async fn eventually<F, Fut>(what: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !condition().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
