use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::readiness::Readiness;

const CONNECTION_LIMIT: Duration = Duration::from_secs(30);
const TEXT: &str = "text/plain; charset=utf-8";
const PROMETHEUS: &str = "text/plain; version=0.0.4; charset=utf-8";

pub struct HttpState {
    pub readiness: Arc<Readiness>,
    pub metrics: Box<dyn Fn() -> String + Send + Sync>,
}

fn reply(status: StatusCode, content_type: &'static str, body: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, content_type)
        .body(Full::new(Bytes::from(body)))
        .unwrap_or_default()
}

pub fn route(state: &HttpState, request: &Request<Incoming>) -> Response<Full<Bytes>> {
    route_of(state, request.method(), request.uri().path())
}

fn route_of(state: &HttpState, method: &Method, path: &str) -> Response<Full<Bytes>> {
    if method != Method::GET {
        return reply(
            StatusCode::METHOD_NOT_ALLOWED,
            TEXT,
            "method not allowed\n".to_owned(),
        );
    }
    match path {
        "/livez" => reply(StatusCode::OK, TEXT, "ok\n".to_owned()),
        "/readyz" => match state.readiness.not_ready_reason() {
            None => reply(StatusCode::OK, TEXT, "ready\n".to_owned()),
            Some(reason) => reply(
                StatusCode::SERVICE_UNAVAILABLE,
                TEXT,
                format!("not ready: {reason}\n"),
            ),
        },
        "/metrics" => reply(StatusCode::OK, PROMETHEUS, (state.metrics)()),
        _ => reply(StatusCode::NOT_FOUND, TEXT, "not found\n".to_owned()),
    }
}

pub async fn serve(listener: TcpListener, state: Arc<HttpState>, shutdown: CancellationToken) {
    loop {
        let stream = tokio::select! {
            () = shutdown.cancelled() => return,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(error) => {
                    tracing::warn!(%error, "a connection to the health port failed");
                    continue;
                }
            },
        };
        let state = state.clone();
        tokio::spawn(async move {
            let service = service_fn(move |request: Request<Incoming>| {
                let state = state.clone();
                async move { Ok::<_, Infallible>(route(&state, &request)) }
            });
            let connection = http1::Builder::new().serve_connection(TokioIo::new(stream), service);
            let _ = tokio::time::timeout(CONNECTION_LIMIT, connection).await;
        });
    }
}

#[cfg(test)]
pub(crate) mod tests;
