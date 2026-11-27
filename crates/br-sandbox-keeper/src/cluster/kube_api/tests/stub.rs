use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use kube::client::Body;
use serde_json::Value;
use tower_service::Service;

use super::*;

#[derive(Debug, Clone)]
pub(super) struct Seen {
    pub method: String,
    pub path: String,
    pub query: String,
    pub content_type: Option<String>,
    pub body: Value,
}

type Route = dyn Fn(&Seen) -> (StatusCode, String) + Send + Sync;

#[derive(Clone)]
pub(super) struct Wire {
    seen: Arc<Mutex<Vec<Seen>>>,
    route: Arc<Route>,
}

impl Wire {
    pub fn routed(route: impl Fn(&Seen) -> (StatusCode, String) + Send + Sync + 'static) -> Self {
        Self {
            seen: Arc::default(),
            route: Arc::new(route),
        }
    }

    pub fn answering(status: StatusCode, reply: Value) -> Self {
        Self::routed(move |_| (status, reply.to_string()))
    }

    pub fn ok(reply: Value) -> Self {
        Self::answering(StatusCode::OK, reply)
    }

    pub fn cluster(&self) -> KubeCluster {
        KubeCluster::new(Client::new(self.clone(), "default"), PodKeys::default())
    }

    pub fn all(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    pub fn only(&self) -> Seen {
        let seen = self.all();
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
        let (seen, route) = (self.seen.clone(), self.route.clone());
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let bytes = body.collect().await.unwrap().to_bytes();
            let request = Seen {
                method: parts.method.to_string(),
                path: parts.uri.path().to_owned(),
                query: parts.uri.query().unwrap_or_default().to_owned(),
                content_type: parts
                    .headers
                    .get("content-type")
                    .map(|value| value.to_str().unwrap().to_owned()),
                body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            };
            let (status, reply) = route(&request);
            seen.lock().unwrap().push(request);
            Ok(Response::builder()
                .status(status)
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(reply)))
                .unwrap())
        })
    }
}
