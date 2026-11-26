mod support;

use br_runner_sandbox::S3Store;
use rustls::crypto::CryptoProvider;

#[test]
fn the_ring_provider_is_installed_when_the_process_has_none_and_the_store_client_is_built() {
    assert!(CryptoProvider::get_default().is_none());

    S3Store::new(support::settings().store()).expect("a store");

    assert!(CryptoProvider::get_default().is_some());
}
