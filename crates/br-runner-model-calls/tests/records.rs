//! One record per call, read on a real JetStream. Run with
//! `cargo test -p br-runner-model-calls -- --ignored --test-threads=1`.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use br_llm_clients_core::Client;
use br_runner_model_calls::{ModelCallRecorder, PublishSettings};
use serde_json::{Value, json};
use support::client::{DIALECT, PROVIDER, REGION, REQUESTED};
use support::{
    Bus, Collecting, Reply, Step, TestCommand, TestServer, context, request, test_client,
};

fn frame(value: Value) -> Step {
    Step::Frame(value)
}

/// A Bedrock Anthropic messages usage block as the client keeps it for a
/// streamed call: `message_start` merged with `message_delta`. `input_tokens`
/// excludes the cache; the cache write detail exists only in `message_start`.
fn bedrock_usage() -> Value {
    json!({
        "input_tokens": 12,
        "cache_creation_input_tokens": 2048,
        "cache_read_input_tokens": 0,
        "cache_creation": {
            "ephemeral_5m_input_tokens": 0,
            "ephemeral_1h_input_tokens": 2048
        },
        "output_tokens": 87,
        "service_tier": "standard",
        "inference_geo": "eu",
        "speed": "standard"
    })
}

/// An Azure OpenAI Responses usage block: `input_tokens` includes the cache.
fn azure_usage() -> Value {
    json!({
        "input_tokens": 3120,
        "input_tokens_details": {
            "cached_tokens": 2048,
            "cache_write_tokens": 1024,
            "cache_write_tokens_details": { "ttl_30m_tokens": 0, "ttl_12h_tokens": 1024 }
        },
        "output_tokens": 210,
        "output_tokens_details": { "reasoning_tokens": 64 },
        "total_tokens": 3330
    })
}

fn success_stream(raw: Value) -> Vec<Step> {
    vec![
        frame(json!({"type": "model", "model": "model-small-1"})),
        frame(json!({"type": "start", "key": "0"})),
        frame(json!({"type": "text", "key": "0", "text": "Hello"})),
        frame(json!({"type": "text", "key": "0", "text": " there"})),
        frame(json!({"type": "end", "key": "0"})),
        frame(json!({"type": "stop"})),
        frame(
            json!({"type": "usage", "input": 12, "output": 87, "cache_read": 0, "cache_write": 2048}),
        ),
        frame(json!({"type": "raw_usage", "usage": raw})),
        frame(json!({"type": "complete"})),
    ]
}

fn fast() -> PublishSettings {
    PublishSettings::new(3, Duration::from_millis(500), Duration::from_millis(50))
        .expect("settings")
}

async fn recorder(bus: &Bus) -> ModelCallRecorder {
    ModelCallRecorder::new(bus.fabric().await, TestCommand::new(), fast())
}

#[tokio::test]
#[ignore = "needs a nats-server binary with JetStream"]
async fn a_successful_call_gives_one_command_with_every_field() {
    let bus = Bus::start().await;
    let server = TestServer::start(Reply::Stream(success_stream(bedrock_usage()))).await;
    let recorder = recorder(&bus).await;
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());
    let before = chrono::Utc::now();

    let mut sink = Collecting::default();
    let outcome = metered.call(&request(), &mut sink).await;
    assert!(outcome.result().is_ok(), "{outcome:?}");
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);
    assert_eq!(recorder.published(), 1);
    assert_eq!(recorder.failed_publishes(), 0);

    let messages = bus.messages().await;
    assert_eq!(messages.len(), 1);
    let message = &messages[0];
    assert_eq!(
        message.subject,
        "integration.cmd.meter.model_call.record.v1"
    );
    let body = &message.body;
    let record = &body["payload"];
    let call_id = record["call_id"].as_str().expect("call id");
    assert_eq!(message.message_id.as_deref(), Some(call_id));
    assert_eq!(body["command_id"].as_str(), Some(call_id));
    let parsed = uuid::Uuid::parse_str(call_id).expect("uuid");
    assert_eq!(parsed.get_version_num(), 7);

    let ctx = context();
    assert_eq!(record["job_id"], json!(ctx.job_id()));
    assert_eq!(record["run_id"], json!(ctx.run_id()));
    assert_eq!(record["attempt"], json!(2));
    assert_eq!(record["initial_job_id"], json!(ctx.initial_job_id()));
    assert_eq!(record["user_id"], json!(ctx.user_id()));
    assert_eq!(
        record["labels"],
        json!({"runner": "ru-example", "step": "draft"})
    );
    assert_eq!(record["provider"], json!(PROVIDER));
    assert_eq!(record["dialect"], json!(DIALECT));
    assert_eq!(record["region"], json!(REGION));
    assert_eq!(record["base_url"], json!(server.url()));
    assert_eq!(record["requested_model"], json!(REQUESTED));
    assert_eq!(record["answered_model"], json!("model-small-1"));
    assert_eq!(record["raw_usage"], bedrock_usage());
    assert_eq!(
        record["usage"],
        json!({"input": 12, "output": 87, "cache_read": 0, "cache_write": 2048})
    );
    assert_eq!(record["service_tier"], json!("standard"));
    assert_eq!(record["inference_geo"], json!("eu"));
    assert_eq!(record["speed"], json!("standard"));
    assert_eq!(record["provider_cost"], Value::Null);
    assert_eq!(record["success"], json!(true));
    assert_eq!(record["abandoned"], json!(false));
    let started: chrono::DateTime<chrono::Utc> =
        serde_json::from_value(record["started_at"].clone()).expect("instant");
    assert!(started >= before && started <= chrono::Utc::now());
    let report = outcome.report();
    assert_eq!(
        record["duration_ms"],
        json!(report.total_duration().as_millis())
    );
    assert_eq!(
        record["time_to_first_token_ms"],
        json!(report.time_to_first_event().map(|d| d.as_millis()))
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary with JetStream"]
async fn the_azure_usage_block_is_carried_whole_with_no_fact_and_a_reported_cost_is_read() {
    let bus = Bus::start().await;
    let server = TestServer::start(Reply::Stream(success_stream(azure_usage()))).await;
    let mut with_cost = azure_usage();
    with_cost["cost"] = json!(0.004213);
    with_cost["service_tier"] = json!(7);
    let costed = TestServer::start(Reply::Stream(success_stream(with_cost.clone()))).await;
    let recorder = recorder(&bus).await;

    for url in [server.url(), costed.url()] {
        let metered = recorder.wrap(Arc::new(test_client(&url)), context());
        let outcome = metered.call(&request(), &mut Collecting::default()).await;
        assert!(outcome.result().is_ok());
        assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);
    }

    let messages = bus.messages().await;
    assert_eq!(messages.len(), 2);
    let plain = &messages[0].body["payload"];
    assert_eq!(plain["raw_usage"], azure_usage());
    for key in ["service_tier", "inference_geo", "speed", "provider_cost"] {
        assert_eq!(plain[key], Value::Null, "{key}");
    }
    let costed = &messages[1].body["payload"];
    assert_eq!(costed["raw_usage"], with_cost);
    assert_eq!(costed["provider_cost"], json!(0.004213));
    assert_eq!(
        costed["service_tier"],
        Value::Null,
        "a number is not a tier"
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary with JetStream"]
async fn a_stream_that_ends_with_an_error_after_a_usage_records_a_failure_with_its_usage() {
    let bus = Bus::start().await;
    let server = TestServer::start(Reply::Stream(vec![
        frame(json!({"type": "start", "key": "0"})),
        frame(json!({"type": "text", "key": "0", "text": "Hel"})),
        frame(json!({"type": "usage", "input": 12, "output": 3})),
        frame(json!({"type": "raw_usage", "usage": {"input_tokens": 12, "output_tokens": 3}})),
        frame(json!({"type": "error", "text": "overloaded", "code": "overloaded_error"})),
    ]))
    .await;
    let recorder = recorder(&bus).await;
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());

    let outcome = metered.call(&request(), &mut Collecting::default()).await;
    assert!(outcome.result().is_err());
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);

    let messages = bus.messages().await;
    assert_eq!(messages.len(), 1);
    let record = &messages[0].body["payload"];
    assert_eq!(record["success"], json!(false));
    assert_eq!(record["abandoned"], json!(false));
    assert_eq!(
        record["raw_usage"],
        json!({"input_tokens": 12, "output_tokens": 3})
    );
    assert_eq!(
        record["usage"],
        json!({"input": 12, "output": 3, "cache_read": null, "cache_write": null})
    );
}

#[tokio::test]
#[ignore = "needs a nats-server binary with JetStream"]
async fn a_call_refused_with_429_records_a_failure_with_no_usage() {
    let bus = Bus::start().await;
    let server = TestServer::start(Reply::Status(
        429,
        r#"{"error":{"type":"rate_limit_error","message":"slow down"}}"#.to_owned(),
    ))
    .await;
    let recorder = recorder(&bus).await;
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());

    let outcome = metered.call(&request(), &mut Collecting::default()).await;
    assert!(outcome.result().is_err());
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);

    let messages = bus.messages().await;
    assert_eq!(messages.len(), 1);
    let record = &messages[0].body["payload"];
    assert_eq!(record["success"], json!(false));
    assert_eq!(record["abandoned"], json!(false));
    assert_eq!(record["raw_usage"], Value::Null);
    assert_eq!(record["usage"], Value::Null);
    assert_eq!(record["time_to_first_token_ms"], Value::Null);
}

#[tokio::test]
#[ignore = "needs a nats-server binary with JetStream"]
async fn a_call_dropped_by_a_timeout_after_its_usage_records_an_abandoned_call() {
    let bus = Bus::start().await;
    let server = TestServer::start(Reply::Stream(vec![
        frame(json!({"type": "model", "model": "model-small-1"})),
        frame(json!({"type": "start", "key": "0"})),
        frame(json!({"type": "text", "key": "0", "text": "Partial"})),
        frame(json!({"type": "usage", "input": 40, "output": 1})),
        frame(json!({"type": "raw_usage", "usage": {"input_tokens": 40, "output_tokens": 1, "service_tier": "priority"}})),
        Step::Pause(Duration::from_secs(10)),
        frame(json!({"type": "stop"})),
    ]))
    .await;
    let recorder = recorder(&bus).await;
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());

    let timeout = Duration::from_millis(400);
    let started = Instant::now();
    let mut sink = Collecting::default();
    let dropped = tokio::time::timeout(timeout, metered.call(&request(), &mut sink)).await;
    assert!(dropped.is_err(), "the timeout drops the call");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);

    let messages = bus.messages().await;
    assert_eq!(messages.len(), 1);
    let record = &messages[0].body["payload"];
    assert_eq!(record["abandoned"], json!(true));
    assert_eq!(record["success"], json!(false));
    assert_eq!(
        record["raw_usage"],
        json!({"input_tokens": 40, "output_tokens": 1, "service_tier": "priority"})
    );
    assert_eq!(record["service_tier"], json!("priority"));
    assert_eq!(
        record["usage"],
        json!({"input": 40, "output": 1, "cache_read": null, "cache_write": null})
    );
    assert_eq!(record["answered_model"], json!("model-small-1"));
    let duration = record["duration_ms"].as_u64().expect("duration");
    assert!(
        (380..1500).contains(&duration),
        "close to the timeout: {duration} ms"
    );
    assert!(record["time_to_first_token_ms"].as_u64().is_some());
}

#[tokio::test]
#[ignore = "needs a nats-server binary with JetStream"]
async fn a_call_dropped_before_any_usage_records_nothing() {
    let bus = Bus::start().await;
    let server = TestServer::start(Reply::Stream(vec![
        frame(json!({"type": "start", "key": "0"})),
        frame(json!({"type": "text", "key": "0", "text": "Partial"})),
        Step::Pause(Duration::from_secs(10)),
    ]))
    .await;
    let recorder = recorder(&bus).await;
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());

    let dropped = tokio::time::timeout(
        Duration::from_millis(300),
        metered.call(&request(), &mut Collecting::default()),
    )
    .await;
    assert!(dropped.is_err());
    assert_eq!(recorder.flush(Duration::from_secs(1)).await, 0);
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(recorder.published(), 0);
    assert_eq!(recorder.failed_publishes(), 0);
    assert!(bus.messages().await.is_empty());
}

#[tokio::test]
#[ignore = "needs a nats-server binary with JetStream"]
async fn two_calls_at_once_on_one_envelope_give_two_records_and_flush_returns_zero() {
    let bus = Bus::start().await;
    let server = TestServer::start(Reply::Stream(success_stream(bedrock_usage()))).await;
    let recorder = recorder(&bus).await;
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());
    let request = request();

    let mut first = Collecting::default();
    let mut second = Collecting::default();
    let (a, b) = tokio::join!(
        metered.call(&request, &mut first),
        metered.call(&request, &mut second)
    );
    assert!(a.result().is_ok() && b.result().is_ok());
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);
    assert_eq!(recorder.published(), 2);

    let messages = bus.messages().await;
    assert_eq!(messages.len(), 2);
    let ids: std::collections::BTreeSet<_> = messages
        .iter()
        .map(|m| {
            m.body["payload"]["call_id"]
                .as_str()
                .expect("id")
                .to_owned()
        })
        .collect();
    assert_eq!(ids.len(), 2, "two call ids");
}

#[tokio::test]
#[ignore = "needs a nats-server binary with JetStream"]
async fn the_bus_drops_a_second_copy_with_the_message_id_of_the_call() {
    let bus = Bus::start().await;
    let server = TestServer::start(Reply::Stream(success_stream(bedrock_usage()))).await;
    let recorder = recorder(&bus).await;
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());
    metered.call(&request(), &mut Collecting::default()).await;
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);

    let messages = bus.messages().await;
    assert_eq!(messages.len(), 1);
    let message_id = messages[0].message_id.clone().expect("a message id");
    let again: br_util_nats_fabric::IntegrationCommand<Value> =
        serde_json::from_value(messages[0].body.clone()).expect("the envelope");
    let outcome = bus
        .fabric()
        .await
        .publish_command_with_id_outcome(&support::coords(), &again, &message_id)
        .await
        .expect("published");
    assert!(outcome.is_duplicate(), "the bus drops the second copy");
    assert_eq!(bus.messages().await.len(), 1);
}
