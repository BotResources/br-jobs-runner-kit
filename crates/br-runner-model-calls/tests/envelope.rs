//! The envelope without a running bus: the deltas, the outcome, the timing of
//! the call, the records it hands off and the counters.

mod support;

use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use br_llm_clients_core::Client;
use br_llm_messages::Usage;
use br_runner_model_calls::{ModelCallRecorder, PublishSettings};
use serde_json::json;
use support::fake::{FakeClient, usage_delta};
use support::nats::unreachable_fabric;
use support::{
    Capturing, CapturingCommand, Collecting, Reply, Step, TestCommand, TestServer,
    UnserializableCommand, context, request, test_client,
};

fn stream() -> Vec<Step> {
    vec![
        Step::Frame(json!({"type": "model", "model": "model-small-1"})),
        Step::Frame(json!({"type": "start", "key": "0"})),
        Step::Frame(json!({"type": "text", "key": "0", "text": "One"})),
        Step::Frame(json!({"type": "text", "key": "0", "text": " two"})),
        Step::Frame(json!({"type": "end", "key": "0"})),
        Step::Frame(json!({"type": "start", "key": "1"})),
        Step::Frame(json!({"type": "text", "key": "1", "text": "three"})),
        Step::Frame(json!({"type": "stop"})),
        Step::Frame(json!({"type": "usage", "input": 9, "output": 3})),
        Step::Frame(json!({"type": "raw_usage", "usage": {"input_tokens": 9, "output_tokens": 3}})),
        Step::Frame(json!({"type": "complete"})),
    ]
}

fn slow_settings() -> PublishSettings {
    PublishSettings::new(3, Duration::from_secs(5), Duration::from_secs(1)).expect("settings")
}

#[tokio::test]
async fn the_deltas_and_the_outcome_are_those_of_the_wrapped_client_alone() {
    let server = TestServer::start(Reply::Stream(stream())).await;
    let recorder = ModelCallRecorder::new(
        unreachable_fabric().await,
        TestCommand::new(),
        slow_settings(),
    );

    let mut alone = Collecting::default();
    let direct = test_client(&server.url())
        .call(&request(), &mut alone)
        .await;
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());
    let mut through = Collecting::default();
    let wrapped = metered.call(&request(), &mut through).await;

    assert!(!alone.deltas.is_empty());
    assert_eq!(through.deltas, alone.deltas, "same deltas, same order");
    assert_eq!(wrapped.result(), direct.result());
    assert_eq!(wrapped.report().usage(), direct.report().usage());
    assert_eq!(wrapped.report().model(), direct.report().model());
}

#[tokio::test]
async fn the_envelope_waits_for_the_callers_sink_as_the_caller_does() {
    let server = TestServer::start(Reply::Stream(stream())).await;
    let recorder = ModelCallRecorder::new(
        unreachable_fabric().await,
        TestCommand::new(),
        slow_settings(),
    );
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());
    let mut sink = Collecting {
        wait: Some(Duration::from_millis(40)),
        ..Collecting::default()
    };
    let started = Instant::now();
    let outcome = metered.call(&request(), &mut sink).await;
    assert!(outcome.result().is_ok());
    let floor = Duration::from_millis(40) * u32::try_from(sink.deltas.len()).expect("count");
    assert!(started.elapsed() >= floor);
}

#[tokio::test]
async fn an_unreachable_bus_never_delays_or_fails_the_call_and_counts_the_failure() {
    let server = TestServer::start(Reply::Stream(stream())).await;
    let settings = PublishSettings::new(2, Duration::from_millis(150), Duration::from_millis(20))
        .expect("settings");
    let recorder = ModelCallRecorder::new(unreachable_fabric().await, TestCommand::new(), settings);
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());

    let started = Instant::now();
    let outcome = metered.call(&request(), &mut Collecting::default()).await;
    assert!(outcome.result().is_ok());
    assert!(
        started.elapsed() < Duration::from_millis(150),
        "{:?}",
        started.elapsed()
    );

    assert_eq!(
        recorder.flush(Duration::from_millis(10)).await,
        1,
        "still in flight"
    );
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);
    assert_eq!(recorder.failed_publishes(), 1);
    assert_eq!(recorder.published(), 0);
}

#[tokio::test]
async fn the_host_command_is_called_once_per_record_and_the_record_is_complete() {
    let server = TestServer::start(Reply::Stream(stream())).await;
    let capture = Capturing::default();
    let recorder = ModelCallRecorder::new(
        unreachable_fabric().await,
        CapturingCommand::new(capture.clone()),
        PublishSettings::new(1, Duration::from_millis(50), Duration::ZERO).expect("settings"),
    );
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());
    let outcome = metered.call(&request(), &mut Collecting::default()).await;
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);

    let records = capture.records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert!(record.success());
    assert!(!record.abandoned());
    assert_eq!(
        record.requested_model().as_str(),
        support::client::REQUESTED
    );
    assert_eq!(
        record.answered_model().map(|m| m.as_str()),
        Some("model-small-1")
    );
    assert_eq!(record.usage(), outcome.report().usage());
    assert_eq!(record.duration(), outcome.report().total_duration());
    assert_eq!(
        record.time_to_first_token(),
        outcome.report().time_to_first_event()
    );
    assert_eq!(
        record.raw_usage().cloned().map(serde_json::Value::Object),
        Some(json!({"input_tokens": 9, "output_tokens": 3}))
    );
    assert_eq!(record.endpoint(), metered.endpoint());
    assert_eq!(record.user_id(), context().user_id());
    assert_eq!(record.labels(), context().labels());
}

#[tokio::test]
async fn a_call_dropped_after_a_usage_delta_records_that_usage_as_abandoned() {
    let capture = Capturing::default();
    let recorder = ModelCallRecorder::new(
        unreachable_fabric().await,
        CapturingCommand::new(capture.clone()),
        PublishSettings::new(1, Duration::from_millis(50), Duration::ZERO).expect("settings"),
    );
    let usage = Usage::of_input_tokens(30).with_output_tokens(2);
    let metered = recorder.wrap(
        Arc::new(FakeClient::new(vec![usage_delta(usage.clone())], true)),
        context(),
    );
    let dropped = tokio::time::timeout(
        Duration::from_millis(100),
        metered.call(&request(), &mut Collecting::default()),
    )
    .await;
    assert!(dropped.is_err());
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);

    let records = capture.records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert!(record.abandoned());
    assert!(!record.success());
    assert_eq!(record.usage(), Some(&usage));
    assert_eq!(record.raw_usage(), None);
    assert_eq!(
        record.answered_model().map(|m| m.as_str()),
        Some("model-from-delta")
    );
    assert!(record.time_to_first_token().is_some());
    assert!(record.duration() >= Duration::from_millis(100));
}

#[tokio::test]
async fn a_call_dropped_with_no_usage_known_records_nothing() {
    let capture = Capturing::default();
    let recorder = ModelCallRecorder::new(
        unreachable_fabric().await,
        CapturingCommand::new(capture.clone()),
        slow_settings(),
    );
    let metered = recorder.wrap(Arc::new(FakeClient::new(Vec::new(), true)), context());
    let dropped = tokio::time::timeout(
        Duration::from_millis(50),
        metered.call(&request(), &mut Collecting::default()),
    )
    .await;
    assert!(dropped.is_err());
    assert_eq!(recorder.flush(Duration::from_millis(100)).await, 0);
    assert!(capture.records().is_empty());
    assert_eq!(recorder.failed_publishes(), 0);
}

#[tokio::test]
async fn a_failed_call_with_no_usage_still_records() {
    let capture = Capturing::default();
    let recorder = ModelCallRecorder::new(
        unreachable_fabric().await,
        CapturingCommand::new(capture.clone()),
        PublishSettings::new(1, Duration::from_millis(20), Duration::ZERO).expect("settings"),
    );
    let metered = recorder.wrap(Arc::new(FakeClient::new(Vec::new(), false)), context());
    let outcome = metered.call(&request(), &mut Collecting::default()).await;
    assert!(outcome.result().is_err());
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);
    let records = capture.records();
    assert_eq!(records.len(), 1);
    assert!(!records[0].success());
    assert!(!records[0].abandoned());
    assert_eq!(records[0].usage(), None);
}

#[tokio::test]
async fn a_payload_that_cannot_be_serialized_is_counted_and_never_attempted() {
    let server = TestServer::start(Reply::Stream(stream())).await;
    let recorder = ModelCallRecorder::new(
        unreachable_fabric().await,
        UnserializableCommand::new(),
        slow_settings(),
    );
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());
    metered.call(&request(), &mut Collecting::default()).await;
    let started = Instant::now();
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "no attempt waited on the bus"
    );
    assert_eq!(recorder.failed_publishes(), 1);
    assert_eq!(recorder.published(), 0);
}

#[tokio::test]
async fn a_host_command_that_panics_is_a_failed_publish_and_the_call_is_unchanged() {
    let server = TestServer::start(Reply::Stream(stream())).await;
    let capture = Capturing {
        panics: true,
        ..Capturing::default()
    };
    let recorder = ModelCallRecorder::new(
        unreachable_fabric().await,
        CapturingCommand::new(capture.clone()),
        slow_settings(),
    );
    let metered = recorder.wrap(Arc::new(test_client(&server.url())), context());
    let outcome = metered.call(&request(), &mut Collecting::default()).await;
    assert!(outcome.result().is_ok());
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);
    assert_eq!(recorder.failed_publishes(), 1);
}

#[test]
fn a_drop_with_no_tokio_runtime_is_counted_as_a_failed_publish() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let fabric = runtime.block_on(unreachable_fabric());
    let recorder = ModelCallRecorder::new(fabric, TestCommand::new(), slow_settings());
    let metered = recorder.wrap(
        Arc::new(FakeClient::new(
            vec![usage_delta(Usage::of_input_tokens(1))],
            true,
        )),
        context(),
    );
    let request = request();
    let mut sink = Collecting::default();
    {
        let mut call = metered.call(&request, &mut sink);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        assert!(matches!(call.as_mut().poll(&mut cx), Poll::Pending));
    }
    assert_eq!(recorder.failed_publishes(), 1);
    assert_eq!(recorder.published(), 0);
    drop(runtime);
}

#[tokio::test]
async fn the_envelope_is_a_shared_client_with_the_settings_and_endpoint_of_the_wrapped_one() {
    let recorder = ModelCallRecorder::new(
        unreachable_fabric().await,
        TestCommand::new(),
        slow_settings(),
    );
    let wrapped = Arc::new(test_client("http://127.0.0.1:9/v1/"));
    let metered = recorder.wrap(wrapped.clone(), context());
    assert_eq!(
        metered.endpoint(),
        br_llm_clients_common_call::ObservedClient::endpoint(wrapped.as_ref())
    );
    assert_eq!(metered.endpoint().region(), Some(support::client::REGION));
    let shared: Arc<dyn Client> = Arc::new(metered);
    assert_eq!(
        shared.settings().model().as_str(),
        support::client::REQUESTED
    );
    fn is_send_sync<T: Send + Sync>(_: &T) {}
    is_send_sync(&shared);
    is_send_sync(&recorder);

    let debug = format!("{recorder:?}");
    assert!(
        debug.contains("integration.cmd.meter.model_call.record.v1"),
        "{debug}"
    );
    assert!(debug.contains("attempts: 3"), "{debug}");
    let clone = recorder.clone();
    assert_eq!(clone.published(), recorder.published());
}

#[tokio::test]
async fn flush_with_nothing_in_flight_returns_zero_at_once() {
    let recorder = ModelCallRecorder::new(
        unreachable_fabric().await,
        TestCommand::new(),
        slow_settings(),
    );
    let started = Instant::now();
    assert_eq!(recorder.flush(Duration::from_secs(5)).await, 0);
    assert!(started.elapsed() < Duration::from_millis(100));
}
