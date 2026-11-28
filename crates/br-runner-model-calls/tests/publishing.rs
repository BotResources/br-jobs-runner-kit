//! Publishing against a bus that stops and comes back. Run with
//! `cargo test -p br-runner-model-calls -- --ignored --test-threads=1`.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use br_llm_clients_core::Client;
use br_runner_model_calls::{ModelCallRecorder, PublishSettings};
use serde_json::json;
use support::{
    Bus, Collecting, Logs, Reply, Step, TestCommand, TestServer, context, request, test_client,
};

fn stream() -> Vec<Step> {
    vec![
        Step::Frame(json!({"type": "start", "key": "0"})),
        Step::Frame(json!({"type": "text", "key": "0", "text": "Hi"})),
        Step::Frame(json!({"type": "stop"})),
        Step::Frame(json!({"type": "usage", "input": 5, "output": 1})),
        Step::Frame(json!({"type": "raw_usage", "usage": {"input_tokens": 5, "output_tokens": 1}})),
        Step::Frame(json!({"type": "complete"})),
    ]
}

fn capture() -> (Logs, tracing::subscriber::DefaultGuard) {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .finish();
    (logs, tracing::subscriber::set_default(subscriber))
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "needs a nats-server binary with JetStream"]
async fn with_the_bus_stopped_the_call_returns_at_once_and_the_failure_is_counted_and_logged() {
    let (logs, _guard) = capture();
    let mut bus = Bus::start().await;
    let server = TestServer::start(Reply::Stream(stream())).await;
    let fabric = bus.fabric().await;
    let settings = PublishSettings::new(3, Duration::from_millis(300), Duration::from_millis(50))
        .expect("settings");
    let recorder = ModelCallRecorder::new(fabric, TestCommand::new(), settings);
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());
    bus.kill();

    let started = Instant::now();
    let outcome = metered.call(&request(), &mut Collecting::default()).await;
    let took = started.elapsed();
    assert!(
        outcome.result().is_ok(),
        "the call does not fail: {outcome:?}"
    );
    assert!(
        took < Duration::from_millis(300),
        "the call does not wait for the bus: {took:?}"
    );
    assert_eq!(
        recorder.failed_publishes(),
        0,
        "the attempts are not spent yet"
    );

    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);
    assert_eq!(recorder.failed_publishes(), 1);
    assert_eq!(recorder.published(), 0);
    let text = logs.text();
    assert!(text.contains("WARN"), "{text}");
    assert!(text.contains("model call record not published"), "{text}");
    assert!(
        text.contains("integration.cmd.meter.model_call.record.v1"),
        "{text}"
    );
    assert!(text.contains(&context().job_id().to_string()), "{text}");
    assert!(
        !text.contains("input_tokens"),
        "no usage in the warning: {text}"
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary with JetStream"]
async fn with_the_bus_back_before_the_last_attempt_the_record_is_published_once() {
    let mut bus = Bus::start().await;
    let server = TestServer::start(Reply::Stream(stream())).await;
    let fabric = bus.fabric().await;
    let settings = PublishSettings::new(10, Duration::from_millis(300), Duration::from_millis(100))
        .expect("settings");
    let recorder = ModelCallRecorder::new(fabric, TestCommand::new(), settings);
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());
    bus.kill();

    let outcome = metered.call(&request(), &mut Collecting::default()).await;
    assert!(outcome.result().is_ok());
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(recorder.published(), 0);
    bus.restart();

    assert_eq!(recorder.flush(Duration::from_secs(15)).await, 0);
    assert_eq!(recorder.published(), 1);
    assert_eq!(recorder.failed_publishes(), 0);
    // a copy buffered by an earlier attempt may reach the server late: wait,
    // then read the stream
    tokio::time::sleep(Duration::from_millis(500)).await;
    let messages = bus.messages().await;
    assert_eq!(messages.len(), 1, "the bus keeps one copy");
}
