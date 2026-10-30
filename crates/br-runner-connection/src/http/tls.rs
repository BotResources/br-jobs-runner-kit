use std::sync::Arc;

use rustls::ClientConfig;
use rustls::RootCertStore;
use rustls::crypto::ring::default_provider;

pub(crate) fn default_config() -> Arc<ClientConfig> {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()
        .expect("the ring provider supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(config)
}
