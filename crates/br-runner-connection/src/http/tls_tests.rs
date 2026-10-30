use std::sync::Arc;
use std::time::Duration;

use http::HeaderValue;
use rcgen::{CertificateParams, CertifiedIssuer, KeyPair};
use rustls::crypto::ring::default_provider;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use super::tls::roots;
use super::{Gateway, default_config};
use crate::endpoint::Endpoint;
use crate::error::{Cause, Error};

struct TlsServer {
    port: u16,
    root: CertificateDer<'static>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TlsServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Identity {
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    root: CertificateDer<'static>,
}

fn self_signed() -> Identity {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).expect("cert");
    let certificate = certified.cert.der().clone();
    Identity {
        chain: vec![certificate.clone()],
        key: PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
            certified.signing_key.serialize_der(),
        )),
        root: certificate,
    }
}

fn signed_by_an_authority() -> Identity {
    let mut authority_params = CertificateParams::new(Vec::new()).expect("authority params");
    authority_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let authority = CertifiedIssuer::self_signed(
        authority_params,
        KeyPair::generate().expect("authority key"),
    )
    .expect("authority");
    let leaf_key = KeyPair::generate().expect("leaf key");
    let leaf = CertificateParams::new(vec!["localhost".to_owned()])
        .expect("leaf params")
        .signed_by(&leaf_key, &authority)
        .expect("leaf");
    Identity {
        chain: vec![leaf.der().clone()],
        key: PrivateKeyDer::from(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
        root: authority.as_ref().der().clone(),
    }
}

async fn serve_tls(identity: Identity) -> TlsServer {
    let Identity { chain, key, root } = identity;
    let config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(chain, key)
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
    TlsServer { port, root, task }
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

async fn post_to(
    host: &str,
    server: &TlsServer,
    tls: Arc<ClientConfig>,
) -> Result<crate::reply::Reply, Error> {
    let endpoint =
        Endpoint::parse(&format!("https://{host}:{}/graphql", server.port)).expect("endpoint");
    Gateway::new(endpoint, tls)
        .post(
            HeaderValue::from_static("Bearer t"),
            b"{}".to_vec(),
            Duration::from_secs(10),
        )
        .await
}

async fn post(server: &TlsServer, tls: Arc<ClientConfig>) -> Result<crate::reply::Reply, Error> {
    post_to("localhost", server, tls).await
}

fn refused_in_transport(result: Result<crate::reply::Reply, Error>) -> String {
    match result {
        Err(Error::Unavailable(failure)) => {
            assert_eq!(failure.cause, Cause::Transport);
            failure.text
        }
        other => panic!("expected Unavailable, got {:?}", other.map(|r| r.status)),
    }
}

#[tokio::test]
async fn an_https_endpoint_whose_certificate_is_trusted_is_served() {
    let server = serve_tls(self_signed()).await;
    let reply = post(&server, trusting(&server.root)).await.expect("served");
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, br#"{"data":{"secure":true}}"#);
}

#[tokio::test]
async fn an_https_endpoint_whose_certificate_is_not_trusted_is_unavailable_by_transport() {
    let server = serve_tls(self_signed()).await;
    refused_in_transport(post(&server, default_config()).await);
}

#[tokio::test]
async fn a_trusted_certificate_is_refused_when_the_endpoint_names_a_host_it_is_not_valid_for() {
    let server = serve_tls(self_signed()).await;
    let tls = trusting(&server.root);
    let text = refused_in_transport(post_to("127.0.0.1", &server, Arc::clone(&tls)).await);
    assert!(text.contains("not valid for name"), "{text}");
    post_to("localhost", &server, tls).await.expect("served");
}

#[tokio::test]
async fn a_certificate_signed_by_an_authority_outside_the_webpki_roots_is_refused() {
    let server = serve_tls(signed_by_an_authority()).await;
    let text = refused_in_transport(post(&server, default_config()).await);
    assert!(
        text.contains("UnknownIssuer") || text.contains("unknown"),
        "{text}"
    );
    post(&server, trusting(&server.root))
        .await
        .expect("served once its authority is trusted");
}

#[test]
fn the_default_trust_roots_are_the_webpki_roots_and_nothing_else() {
    let roots = roots();
    assert!(!roots.roots.is_empty());
    assert_eq!(roots.roots, webpki_roots::TLS_SERVER_ROOTS.to_vec());
}
