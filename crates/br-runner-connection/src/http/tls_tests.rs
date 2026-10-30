use std::sync::Arc;
use std::time::Duration;

use http::HeaderValue;
use rustls::crypto::ring::default_provider;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use super::{Gateway, default_config};
use crate::endpoint::Endpoint;
use crate::error::{Cause, Error};

struct TlsServer {
    port: u16,
    certificate: CertificateDer<'static>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TlsServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve_tls() -> TlsServer {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).expect("cert");
    let certificate = certified.cert.der().clone();
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    let config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(vec![certificate.clone()], key)
        .expect("server config");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let task = tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut stream) = acceptor.accept(tcp).await else {
                    return;
                };
                let mut buffer = vec![0u8; 4096];
                let _ = stream.read(&mut buffer).await;
                let body = br#"{"data":{"secure":true}}"#;
                let head = format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n", body.len());
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(body).await;
                let _ = stream.flush().await;
            });
        }
    });
    TlsServer {
        port,
        certificate,
        task,
    }
}

fn trusting(certificate: &CertificateDer<'static>) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.add(certificate.clone()).expect("root");
    Arc::new(
        ClientConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()
            .expect("versions")
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

async fn post(server: &TlsServer, tls: Arc<ClientConfig>) -> Result<crate::reply::Reply, Error> {
    let endpoint =
        Endpoint::parse(&format!("https://localhost:{}/graphql", server.port)).expect("endpoint");
    Gateway::new(endpoint, tls)
        .post(
            HeaderValue::from_static("Bearer t"),
            b"{}".to_vec(),
            Duration::from_secs(10),
        )
        .await
}

#[tokio::test]
async fn an_https_endpoint_whose_certificate_is_trusted_is_served() {
    let server = serve_tls().await;
    let reply = post(&server, trusting(&server.certificate))
        .await
        .expect("served");
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, br#"{"data":{"secure":true}}"#);
}

#[tokio::test]
async fn an_https_endpoint_whose_certificate_is_not_trusted_is_unavailable_by_transport() {
    let server = serve_tls().await;
    match post(&server, default_config()).await {
        Err(Error::Unavailable(failure)) => assert_eq!(failure.cause, Cause::Transport),
        other => panic!("expected Unavailable, got {:?}", other.map(|r| r.status)),
    }
}
