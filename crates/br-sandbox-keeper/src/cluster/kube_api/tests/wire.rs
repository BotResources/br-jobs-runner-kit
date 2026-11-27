use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use kube::client::Body;
use serde_json::{Value, json};
use tower_service::Service;

use super::*;
use crate::cluster::{DeleteOutcome, EventTarget, KubeApi, PoolEvent};
use crate::facts::PodVersion;
use crate::rules::ConditionWrite;

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    query: String,
    content_type: Option<String>,
    body: Value,
}

#[derive(Clone)]
struct Wire {
    seen: Arc<Mutex<Vec<Seen>>>,
    status: StatusCode,
    reply: Arc<Value>,
}

impl Wire {
    fn answering(status: StatusCode, reply: Value) -> Self {
        Self {
            seen: Arc::default(),
            status,
            reply: Arc::new(reply),
        }
    }

    fn ok(reply: Value) -> Self {
        Self::answering(StatusCode::OK, reply)
    }

    fn cluster(&self) -> KubeCluster {
        KubeCluster::new(Client::new(self.clone(), "default"), PodKeys::default())
    }

    fn only(&self) -> Seen {
        let seen = self.seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "{seen:?}");
        seen[0].clone()
    }
}

impl Service<Request<Body>> for Wire {
    type Response = Response<Full<Bytes>>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let (seen, status, reply) = (self.seen.clone(), self.status, self.reply.clone());
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let bytes = body.collect().await.unwrap().to_bytes();
            seen.lock().unwrap().push(Seen {
                method: parts.method.to_string(),
                path: parts.uri.path().to_owned(),
                query: parts.uri.query().unwrap_or_default().to_owned(),
                content_type: parts
                    .headers
                    .get("content-type")
                    .map(|value| value.to_str().unwrap().to_owned()),
                body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            });
            Ok(Response::builder()
                .status(status)
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(reply.to_string())))
                .unwrap())
        })
    }
}

fn replica_set_reply() -> Value {
    json!({"apiVersion": "apps/v1", "kind": "ReplicaSet", "metadata": {"name": "warm-p"}})
}

fn status_reply(code: u16, reason: &str) -> Value {
    json!({"apiVersion": "v1", "kind": "Status", "status": "Failure", "message": reason,
        "reason": reason, "code": code})
}

fn version() -> PodVersion {
    PodVersion {
        uid: "uid-7".to_owned(),
        resource_version: "42".to_owned(),
    }
}

#[tokio::test]
async fn a_delete_carries_the_uid_and_the_resource_version_of_the_last_read() {
    let wire = Wire::ok(json!({"apiVersion": "v1", "kind": "Status", "status": "Success"}));
    let outcome = wire.cluster().delete_pod("sbx", "w0", &version()).await;
    assert_eq!(outcome, Ok(DeleteOutcome::Deleted));
    let seen = wire.only();
    assert_eq!(seen.method, "DELETE");
    assert_eq!(seen.path, "/api/v1/namespaces/sbx/pods/w0");
    assert_eq!(
        seen.body["preconditions"],
        json!({"uid": "uid-7", "resourceVersion": "42"})
    );
}

#[tokio::test]
async fn a_delete_that_the_server_refuses_as_a_conflict_or_as_gone_is_not_an_error() {
    let conflict = Wire::answering(StatusCode::CONFLICT, status_reply(409, "Conflict"));
    assert_eq!(
        conflict.cluster().delete_pod("sbx", "w0", &version()).await,
        Ok(DeleteOutcome::Conflict)
    );
    let gone = Wire::answering(StatusCode::NOT_FOUND, status_reply(404, "NotFound"));
    assert_eq!(
        gone.cluster().delete_pod("sbx", "w0", &version()).await,
        Ok(DeleteOutcome::Gone)
    );
    let broken = Wire::answering(StatusCode::INTERNAL_SERVER_ERROR, status_reply(500, "Oops"));
    assert!(matches!(
        broken.cluster().delete_pod("sbx", "w0", &version()).await,
        Err(ApiError::Call(_))
    ));
}

#[tokio::test]
async fn the_replicas_are_written_by_a_server_side_apply_of_the_one_field_under_the_field_manager()
{
    let wire = Wire::ok(replica_set_reply());
    wire.cluster()
        .apply_replicas("sbx", "warm-p", 7)
        .await
        .unwrap();
    let seen = wire.only();
    assert_eq!(seen.method, "PATCH");
    assert_eq!(seen.path, "/apis/apps/v1/namespaces/sbx/replicasets/warm-p");
    assert!(seen.query.contains("fieldManager=br-sandbox-keeper"));
    assert!(seen.query.contains("force=true"));
    assert_eq!(
        seen.content_type.as_deref(),
        Some("application/apply-patch+yaml")
    );
    assert_eq!(
        seen.body,
        json!({"apiVersion": "apps/v1", "kind": "ReplicaSet",
            "metadata": {"name": "warm-p"}, "spec": {"replicas": 7}})
    );
}

#[tokio::test]
async fn the_condition_is_written_on_the_status_and_nothing_else_of_the_status() {
    let wire = Wire::ok(replica_set_reply());
    let write = ConditionWrite {
        status: "True",
        reason: "ImagePullFailing",
        message: "1 warm pod(s) failing; kubelet reason: ErrImagePull".to_owned(),
        last_transition_time: at(1_700_000_000),
    };
    wire.cluster()
        .apply_condition("sbx", "warm-p", &write)
        .await
        .unwrap();
    let seen = wire.only();
    assert_eq!(seen.method, "PATCH");
    assert_eq!(
        seen.path,
        "/apis/apps/v1/namespaces/sbx/replicasets/warm-p/status"
    );
    assert!(seen.query.contains("fieldManager=br-sandbox-keeper"));
    assert_eq!(
        seen.body["status"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        vec!["conditions"]
    );
    let conditions = seen.body["status"]["conditions"].as_array().unwrap();
    assert_eq!(conditions.len(), 1);
    assert_eq!(conditions[0]["type"], "WarmPodsFailing");
    assert_eq!(conditions[0]["status"], "True");
    assert_eq!(conditions[0]["reason"], "ImagePullFailing");
    assert!(seen.body.get("spec").is_none());
}

#[tokio::test]
async fn an_event_names_the_replica_set_and_carries_the_reason() {
    let wire = Wire::ok(json!({"metadata": {"name": "e"}, "involvedObject": {}}));
    let target = EventTarget {
        name: "warm-p".to_owned(),
        uid: "rs-uid".to_owned(),
    };
    wire.cluster()
        .create_event("sbx", &target, PoolEvent::AtMax, "above the maximum")
        .await
        .unwrap();
    let seen = wire.only();
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.path, "/api/v1/namespaces/sbx/events");
    assert_eq!(seen.body["reason"], "PoolAtMax");
    assert_eq!(seen.body["message"], "above the maximum");
    assert_eq!(seen.body["involvedObject"]["kind"], "ReplicaSet");
    assert_eq!(seen.body["involvedObject"]["name"], "warm-p");
    assert_eq!(seen.body["involvedObject"]["uid"], "rs-uid");
    assert_eq!(seen.body["involvedObject"]["namespace"], "sbx");
}

#[tokio::test]
async fn the_managed_namespaces_are_the_ones_that_match_the_selector() {
    let wire = Wire::ok(json!({"apiVersion": "v1", "kind": "NamespaceList", "metadata": {},
        "items": [{"metadata": {"name": "a"}}, {"metadata": {"name": "b"}}]}));
    let found = wire
        .cluster()
        .managed_namespaces("br.sandbox/managed=true")
        .await;
    assert_eq!(found, Ok(vec!["a".to_owned(), "b".to_owned()]));
    let seen = wire.only();
    assert_eq!(seen.method, "GET");
    assert_eq!(seen.path, "/api/v1/namespaces");
    assert!(
        seen.query
            .contains("labelSelector=br.sandbox%2Fmanaged%3Dtrue")
    );
}
