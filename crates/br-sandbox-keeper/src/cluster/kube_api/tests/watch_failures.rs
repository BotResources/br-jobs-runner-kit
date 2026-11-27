use http::StatusCode;
use serde_json::json;

use super::reads::{pod_list, watch_line};
use super::stub::Wire;
use super::*;
use crate::cluster::KubeApi;

fn refused() -> String {
    json!({"apiVersion": "v1", "kind": "Status", "status": "Failure",
        "message": "the server is down", "reason": "InternalError", "code": 500})
    .to_string()
}

fn failure_of(event: &PodEvent) -> &str {
    match event {
        PodEvent::WatchFailed { reason } => reason,
        other => panic!("{other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn a_failed_list_gives_a_failure_and_the_list_is_tried_again() {
    let wire = Wire::routed(|_| (StatusCode::INTERNAL_SERVER_ERROR, refused()));
    let events: Vec<PodEvent> = wire.cluster().watch_pods("sbx").take(2).collect().await;
    for event in &events {
        let reason = failure_of(event);
        assert!(reason.starts_with("the pod list failed"), "{reason}");
        assert!(reason.contains("the server is down"), "{reason}");
    }
    assert_eq!(wire.all().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_watch_that_cannot_be_opened_gives_the_snapshot_then_a_failure_and_a_new_list() {
    let wire = Wire::routed(|seen| {
        if seen.query.contains("watch=true") {
            (StatusCode::INTERNAL_SERVER_ERROR, refused())
        } else {
            (StatusCode::OK, pod_list().to_string())
        }
    });
    let events: Vec<PodEvent> = wire.cluster().watch_pods("sbx").take(3).collect().await;
    assert!(matches!(&events[0], PodEvent::Snapshot(pods) if pods.len() == 1));
    assert!(failure_of(&events[1]).starts_with("the pod watch could not be opened"));
    assert!(matches!(&events[2], PodEvent::Snapshot(_)));
    let lists = wire
        .all()
        .iter()
        .filter(|seen| !seen.query.contains("watch=true"))
        .count();
    assert_eq!(lists, 2);
}

#[tokio::test(start_paused = true)]
async fn an_error_event_of_the_watch_gives_a_failure_and_a_new_list() {
    let expired = json!({"type": "ERROR", "object": {"apiVersion": "v1", "kind": "Status",
        "status": "Failure", "message": "too old resource version", "reason": "Expired",
        "code": 410}});
    let wire = Wire::routed(move |seen| {
        if seen.query.contains("watch=true") {
            (StatusCode::OK, format!("{expired}\n"))
        } else {
            (StatusCode::OK, pod_list().to_string())
        }
    });
    let events: Vec<PodEvent> = wire.cluster().watch_pods("sbx").take(3).collect().await;
    assert!(matches!(&events[0], PodEvent::Snapshot(_)));
    let reason = failure_of(&events[1]);
    assert!(reason.contains("410 Expired"), "{reason}");
    assert!(reason.contains("too old resource version"), "{reason}");
    assert!(matches!(&events[2], PodEvent::Snapshot(_)));
}

#[tokio::test(start_paused = true)]
async fn a_watch_line_that_cannot_be_read_gives_a_failure_and_a_new_list() {
    let wire = Wire::routed(|seen| {
        if seen.query.contains("watch=true") {
            (StatusCode::OK, "{not json}\n".to_owned())
        } else {
            (StatusCode::OK, pod_list().to_string())
        }
    });
    let events: Vec<PodEvent> = wire.cluster().watch_pods("sbx").take(3).collect().await;
    assert!(matches!(&events[0], PodEvent::Snapshot(_)));
    assert!(failure_of(&events[1]).starts_with("the pod watch broke"));
    assert!(matches!(&events[2], PodEvent::Snapshot(_)));
}

#[tokio::test(start_paused = true)]
async fn a_watch_that_ends_normally_is_opened_again_with_no_failure() {
    let wire = Wire::routed(|seen| {
        if seen.query.contains("watch=true") {
            (StatusCode::OK, watch_line())
        } else {
            (StatusCode::OK, pod_list().to_string())
        }
    });
    let events: Vec<PodEvent> = wire.cluster().watch_pods("sbx").take(3).collect().await;
    assert!(matches!(&events[0], PodEvent::Snapshot(_)));
    assert!(matches!(&events[1], PodEvent::Applied(_)));
    assert!(matches!(&events[2], PodEvent::Applied(_)));
}
