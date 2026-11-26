use rustls::crypto::{CryptoProvider, ring};

pub(crate) fn ensure_crypto_provider() {
    if CryptoProvider::get_default().is_none() {
        let _ = ring::default_provider().install_default();
    }
}
