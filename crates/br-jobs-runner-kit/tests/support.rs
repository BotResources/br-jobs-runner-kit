use std::time::Duration;

use async_nats::jetstream::consumer::pull;
use async_nats::jetstream::{Context, kv, stream};
use br_jobs_runner_kit::{HarnessError, InstanceKey, Runner, RunnerConfig, RunnerType, run_until};
use contract_jobs::runner::{
    CANCEL_BUCKET, LOG_FILTER, LOG_STREAM, PRESENCE_BUCKET, STATUS_FILTER, STATUS_STREAM,
    TRIGGER_FILTER, TRIGGER_STREAM, Trigger, WIRE_VERSION,
};
use contract_jobs::runner_transport::{run_cancel_key, trigger_subject};
use contract_jobs::segment::SubjectSegment;
use futures::StreamExt;
use serde_json::Value;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

pub const PRESENCE_TTL: Duration = Duration::from_secs(4);

pub fn nats_url() -> String {
    std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_owned())
}

pub async fn jetstream() -> Context {
    let client = async_nats::connect(nats_url())
        .await
        .expect("a nats-server is reachable for the e2e suite");
    async_nats::jetstream::new(client)
}

pub async fn provision(js: &Context) {
    for (name, filter) in [
        (TRIGGER_STREAM, TRIGGER_FILTER),
        (STATUS_STREAM, STATUS_FILTER),
        (LOG_STREAM, LOG_FILTER),
    ] {
        let _ = js.delete_stream(name).await;
        js.create_stream(stream::Config {
            name: name.to_owned(),
            subjects: vec![filter.to_owned()],
            ..Default::default()
        })
        .await
        .unwrap_or_else(|error| panic!("provisioning the {name} stream: {error}"));
    }
    let _ = js.delete_key_value(CANCEL_BUCKET).await;
    js.create_key_value(kv::Config {
        bucket: CANCEL_BUCKET.to_owned(),
        ..Default::default()
    })
    .await
    .expect("provisioning the cancel bucket");
    let _ = js.delete_key_value(PRESENCE_BUCKET).await;
    js.create_key_value(kv::Config {
        bucket: PRESENCE_BUCKET.to_owned(),
        max_age: PRESENCE_TTL,
        history: 1,
        ..Default::default()
    })
    .await
    .expect("provisioning the presence bucket");
}

pub fn config(runner_type: &str, instance_key: &str) -> RunnerConfig {
    let mut config = RunnerConfig::new(
        nats_url(),
        RunnerType::new(runner_type).expect("a valid runner type"),
        InstanceKey::new(instance_key).expect("a valid instance key"),
        "e2e-0.1.0",
    );
    config.drain_timeout = Duration::from_secs(5);
    config
}

pub struct RunningHarness {
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<(), HarnessError>>,
}

impl RunningHarness {
    pub fn start<R: Runner>(config: RunnerConfig, runner: R) -> Self {
        let (stop, stopped) = oneshot::channel::<()>();
        let task = tokio::spawn(run_until(config, runner, async move {
            let _ = stopped.await;
        }));
        Self {
            stop: Some(stop),
            task,
        }
    }

    pub async fn shut_down(mut self) -> Result<(), HarnessError> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .expect("the harness drains within its timeout")
            .expect("the harness task does not panic")
    }

    pub async fn outcome(self) -> Result<(), HarnessError> {
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .expect("the harness settles within the timeout")
            .expect("the harness task does not panic")
    }
}

pub struct StreamProbe {
    messages: pull::Stream,
}

impl StreamProbe {
    pub async fn on_status(js: &Context, runner_type: &str) -> Self {
        Self::open(js, STATUS_STREAM, format!("jobs.status.{runner_type}.>")).await
    }

    pub async fn on_logs(js: &Context, runner_type: &str) -> Self {
        Self::open(js, LOG_STREAM, format!("jobs.log.{runner_type}")).await
    }

    async fn open(js: &Context, stream: &str, filter: String) -> Self {
        let stream = js
            .get_stream(stream)
            .await
            .expect("the stream is provisioned");
        let consumer = stream
            .create_consumer(pull::Config {
                filter_subject: filter,
                ..Default::default()
            })
            .await
            .expect("an ephemeral probe consumer");
        let messages = consumer
            .stream()
            .max_messages_per_batch(1)
            .messages()
            .await
            .expect("the probe pull stream opens");
        Self { messages }
    }

    pub async fn next_fact(&mut self, timeout: Duration) -> Option<(String, Value)> {
        let message = tokio::time::timeout(timeout, self.messages.next())
            .await
            .ok()??
            .expect("the probe reads its stream");
        let fact = message
            .subject
            .split('.')
            .next_back()
            .expect("a subject has segments")
            .to_owned();
        let payload = serde_json::from_slice(&message.payload).expect("a wire fact is JSON");
        message.ack().await.expect("the probe acks");
        Some((fact, payload))
    }

    pub async fn expect_fact(&mut self, expected: &str, timeout: Duration) -> Value {
        let (fact, payload) = self
            .next_fact(timeout)
            .await
            .unwrap_or_else(|| panic!("no '{expected}' fact within {timeout:?}"));
        assert_eq!(fact, expected, "unexpected fact order: {payload}");
        payload
    }

    pub async fn expect_quiet(&mut self, quiet: Duration) {
        if let Some((fact, payload)) = self.next_fact(quiet).await {
            panic!("expected silence, got '{fact}': {payload}");
        }
    }
}

pub async fn publish_trigger(js: &Context, runner_type: &str, run_id: Uuid, config: Value) {
    let segment = SubjectSegment::runner_type(runner_type).expect("a valid runner type");
    let trigger = Trigger {
        version: WIRE_VERSION,
        run_id,
        job_id: Uuid::now_v7(),
        config: Some(config),
        attempt: 1,
        triggered_by: None,
    };
    js.publish(
        trigger_subject(&segment),
        serde_json::to_vec(&trigger)
            .expect("a trigger serializes")
            .into(),
    )
    .await
    .expect("publishing the trigger")
    .await
    .expect("the trigger lands on its stream");
}

pub async fn cancel_run(js: &Context, run_id: Uuid) {
    let store = js
        .get_key_value(CANCEL_BUCKET)
        .await
        .expect("the cancel bucket is provisioned");
    store
        .put(
            run_cancel_key(run_id),
            serde_json::to_vec(&serde_json::json!({ "version": WIRE_VERSION, "run_id": run_id }))
                .expect("a cancel entry serializes")
                .into(),
        )
        .await
        .expect("writing the cancel entry");
}

pub async fn presence_entry(js: &Context, runner_type: &str, instance_key: &str) -> Option<Value> {
    let store = js
        .get_key_value(PRESENCE_BUCKET)
        .await
        .expect("the presence bucket is provisioned");
    store
        .get(format!("{runner_type}.{instance_key}"))
        .await
        .expect("reading the presence bucket")
        .map(|bytes| serde_json::from_slice(&bytes).expect("a presence entry is JSON"))
}

pub async fn wait_for_presence(js: &Context, runner_type: &str, instance_key: &str) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(entry) = presence_entry(js, runner_type, instance_key).await {
            return entry;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no presence entry for {runner_type}.{instance_key} within 5s"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub async fn wait_for_absence(js: &Context, runner_type: &str, instance_key: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if presence_entry(js, runner_type, instance_key)
            .await
            .is_none()
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the presence entry for {runner_type}.{instance_key} was not deleted within 5s"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
