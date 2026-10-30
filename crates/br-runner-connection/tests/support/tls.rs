use std::sync::Arc;

use rustls::ServerConfig;
use rustls::crypto::ring::default_provider;
use rustls_pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

pub struct UntrustedTlsServer {
    port: u16,
    task: JoinHandle<()>,
}

impl UntrustedTlsServer {
    pub async fn start() -> Self {
        let certified =
            rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).expect("cert");
        let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
            certified.signing_key.serialize_der(),
        ));
        let config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
            .with_safe_default_protocol_versions()
            .expect("versions")
            .with_no_client_auth()
            .with_single_cert(vec![certified.cert.der().clone()], key)
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
                    let _ = acceptor.accept(tcp).await;
                });
            }
        });
        Self { port, task }
    }

    pub fn url(&self) -> String {
        format!("https://localhost:{}/graphql", self.port)
    }
}

impl Drop for UntrustedTlsServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
