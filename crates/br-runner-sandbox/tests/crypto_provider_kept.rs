use std::sync::Arc;

mod support;

use br_runner_sandbox::S3Store;
use rustls::crypto::{CryptoProvider, ring};

#[test]
fn a_crypto_provider_set_by_the_caller_is_kept_when_the_store_client_is_built() {
    let base = ring::default_provider();
    let narrowed = CryptoProvider {
        cipher_suites: base.cipher_suites.iter().take(1).copied().collect(),
        ..base
    };
    narrowed
        .install_default()
        .expect("no provider is set in this process");
    let before = CryptoProvider::get_default().expect("the provider of the caller");

    S3Store::new(support::settings().store()).expect("a store");

    let after = CryptoProvider::get_default().expect("a provider");
    assert!(Arc::ptr_eq(before, after));
    assert_eq!(after.cipher_suites.len(), 1);
}
