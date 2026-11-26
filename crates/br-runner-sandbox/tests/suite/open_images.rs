use std::time::Duration;

use br_agentic_harness_contract::{OpenFault, PortError, SandboxPort};
use br_runner_sandbox::{Error, ImageError};
use serde_json::json;

use crate::fakes::{Capture, Lineage, boxes, boxes_with, failing, fresh_request, pool};

const REASON: &str = "ImagePullFailing";
const MESSAGE: &str = "back-off pulling image agent:9";

#[tokio::test(start_paused = true)]
async fn image_failing_the_port_gives_the_typed_cause_that_the_accessor_reads_and_the_values_go_to_the_log()
 {
    let capture = Capture::install();
    let boxes = boxes();
    boxes
        .cluster
        .set_replica_set(Some(pool(vec![failing(REASON, MESSAGE)])));

    let error = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("a fault");

    assert!(matches!(error, PortError::Fault(_)));
    assert_eq!(OpenFault::of(&error), Some(OpenFault::ImageFailing));
    let lines = capture.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("WARN"), "{lines:?}");
    assert!(
        lines[0].contains(REASON) && lines[0].contains(MESSAGE),
        "{lines:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn image_unavailable_the_port_gives_the_typed_cause_and_the_wait_that_ended_goes_to_the_log()
{
    let capture = Capture::install();
    let boxes = boxes_with(json!({"open_timeout_seconds": 10}));
    boxes.cluster.ready_on_create(false);

    let error = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("a fault");

    assert_eq!(OpenFault::of(&error), Some(OpenFault::ImageUnavailable));
    let lines = capture.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("waited_ms=10000"), "{lines:?}");
}

#[tokio::test(start_paused = true)]
async fn image_errors_the_port_error_holds_no_field_so_the_values_are_not_in_the_port_error() {
    let boxes = boxes();
    boxes
        .cluster
        .set_replica_set(Some(pool(vec![failing(REASON, MESSAGE)])));

    let error = boxes
        .sandbox
        .open(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("a fault");

    let shown = format!("{error:?} {error}");
    assert!(
        !shown.contains(REASON) && !shown.contains(MESSAGE),
        "{shown}"
    );
}

#[tokio::test(start_paused = true)]
async fn image_failing_a_caller_that_holds_the_adapter_reads_the_reason_and_the_message_from_open_box()
 {
    let boxes = boxes();
    boxes
        .cluster
        .set_replica_set(Some(pool(vec![failing(REASON, MESSAGE)])));

    let error = boxes
        .sandbox
        .open_box(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("an error");

    assert!(matches!(
        error,
        Error::Image(ImageError::Failing { ref reason, ref message })
            if reason == REASON && message == MESSAGE
    ));
}

#[tokio::test(start_paused = true)]
async fn image_unavailable_a_caller_that_holds_the_adapter_reads_the_wait_from_open_box() {
    let boxes = boxes_with(json!({"open_timeout_seconds": 10}));
    boxes.cluster.ready_on_create(false);

    let error = boxes
        .sandbox
        .open_box(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("an error");

    assert!(matches!(
        error,
        Error::Image(ImageError::Unavailable { waited }) if waited == Duration::from_secs(10)
    ));
}

#[tokio::test(start_paused = true)]
async fn image_errors_a_pool_with_no_pod_ready_in_the_time_is_image_unavailable_and_never_a_capacity_error()
 {
    let boxes = boxes_with(json!({"cold_create": false}));

    let error = boxes
        .sandbox
        .open_box(&fresh_request(Lineage::new().first()))
        .await
        .expect_err("an error");

    assert!(
        matches!(error, Error::Image(ImageError::Unavailable { waited }) if waited == Duration::ZERO)
    );
}
