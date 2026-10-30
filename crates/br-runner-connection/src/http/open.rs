use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use rustls::ClientConfig;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use super::chain;
use crate::endpoint::Endpoint;
use crate::error::{Error, Failure};

pub(super) trait Io: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}

pub(super) type Stream = TokioIo<Box<dyn Io>>;
pub(super) type Sender = http1::SendRequest<Full<Bytes>>;
pub(super) type Driver = http1::Connection<Stream, Full<Bytes>>;

fn unavailable(error: &dyn std::error::Error) -> Error {
    Error::Unavailable(Failure::transport(chain(error)))
}

pub(super) async fn open(
    endpoint: &Endpoint,
    tls: &Arc<ClientConfig>,
) -> Result<(Sender, Driver), Error> {
    let tcp = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        .await
        .map_err(|e| unavailable(&e))?;
    let io: Box<dyn Io> = if endpoint.secure {
        let secured = TlsConnector::from(Arc::clone(tls))
            .connect(endpoint.server_name.clone(), tcp)
            .await
            .map_err(|e| unavailable(&e))?;
        Box::new(secured)
    } else {
        Box::new(tcp)
    };
    http1::handshake(TokioIo::new(io))
        .await
        .map_err(|e| unavailable(&e))
}
