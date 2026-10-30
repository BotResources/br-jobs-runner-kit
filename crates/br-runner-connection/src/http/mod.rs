mod open;
mod tls;
#[cfg(test)]
mod tls_tests;

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
use http::{HeaderValue, Method, Request};
use http_body_util::{BodyExt, Full};
use rustls::ClientConfig;
use tokio::time::error::Elapsed;
use tokio::time::{Instant, timeout_at};

use crate::endpoint::Endpoint;
use crate::error::{Error, Failure};
use crate::reply::Reply;
use open::{Driver, Sender, open};

pub(crate) use tls::default_config;

pub(crate) fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

async fn before<T>(deadline: Option<Instant>, work: impl Future<Output = T>) -> Result<T, Elapsed> {
    match deadline {
        Some(at) => timeout_at(at, work).await,
        None => Ok(work.await),
    }
}

fn lost(error: &dyn std::error::Error) -> Error {
    Error::Lost(Failure::transport(chain(error)))
}

#[derive(Clone)]
pub(crate) struct Gateway {
    endpoint: Endpoint,
    tls: Arc<ClientConfig>,
}

impl Gateway {
    pub(crate) fn new(endpoint: Endpoint, tls: Arc<ClientConfig>) -> Self {
        Self { endpoint, tls }
    }

    pub(crate) async fn post(
        &self,
        authorization: HeaderValue,
        body: Vec<u8>,
        wait: Duration,
    ) -> Result<Reply, Error> {
        let request = self.request(authorization, body)?;
        let deadline = Instant::now().checked_add(wait);
        let (sender, driver) = before(deadline, open(&self.endpoint, &self.tls))
            .await
            .map_err(|_| {
                Error::Unavailable(Failure::wait(
                    "the wait fired before the connection was ready",
                ))
            })??;
        before(deadline, exchange(sender, driver, request))
            .await
            .map_err(|_| Error::Lost(Failure::wait("the wait fired before the answer")))?
    }

    pub(crate) fn request(
        &self,
        authorization: HeaderValue,
        body: Vec<u8>,
    ) -> Result<Request<Full<Bytes>>, Error> {
        Request::builder()
            .method(Method::POST)
            .uri(self.endpoint.target.as_str())
            .header(HOST, self.endpoint.host_header.clone())
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .header(AUTHORIZATION, authorization)
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| Error::InvalidInput(e.to_string()))
    }
}

async fn roundtrip(sender: &mut Sender, request: Request<Full<Bytes>>) -> Result<Reply, Error> {
    let response = sender.send_request(request).await.map_err(|e| lost(&e))?;
    let status = response.status().as_u16();
    if status == 401 {
        return Ok(Reply {
            status,
            body: Vec::new(),
        });
    }
    match response.into_body().collect().await {
        Ok(collected) => Ok(Reply {
            status,
            body: collected.to_bytes().to_vec(),
        }),
        Err(_) if status == 403 => Ok(Reply {
            status,
            body: Vec::new(),
        }),
        Err(error) => Err(lost(&error)),
    }
}

async fn exchange(
    mut sender: Sender,
    driver: Driver,
    request: Request<Full<Bytes>>,
) -> Result<Reply, Error> {
    let mut driver = pin!(driver);
    let mut answer = pin!(roundtrip(&mut sender, request));
    tokio::select! {
        biased;
        result = &mut answer => result,
        ended = &mut driver => {
            let result = answer.await;
            match (result, ended) {
                (Ok(reply), _) => Ok(reply),
                (Err(_), Err(error)) => Err(lost(&error)),
                (Err(error), Ok(())) => Err(error),
            }
        }
    }
}
