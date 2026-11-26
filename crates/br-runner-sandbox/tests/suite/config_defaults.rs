use std::time::Duration;

use br_runner_sandbox::{NetworkMode, NetworkRule};
use serde_json::json;

use crate::common::{build, config, input, required, with, without};

#[test]
fn every_default_of_the_table_is_the_value_of_the_table() {
    let config = config();
    assert_eq!(config.kube_context(), None);
    assert_eq!(config.label_prefix().as_str(), "br.sandbox");
    assert_eq!(config.network(), &NetworkRule::None);
    assert_eq!(
        config
            .save_scope_paths()
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        vec!["/work", "/home/agent"]
    );
    assert_eq!(config.save_zstd_level(), 3);
    assert_eq!(config.restore_timeout(), Duration::from_secs(30));
    assert_eq!(config.open_timeout(), Duration::from_secs(30));
    assert_eq!(config.exec_timeout(), None);
    assert_eq!(config.save_timeout(), None);
    assert_eq!(config.idle_ttl(), Duration::from_secs(120));
    assert_eq!(config.idle_keep_min_state_bytes(), 10_000_000);
    assert_eq!(config.claim_max_attempts(), 20);
    assert!(config.cold_create());
    assert_eq!(config.agent_container(), "sandbox");
    assert_eq!(config.seed_container(), "seed");
    assert_eq!(config.tools_path().as_str(), "/opt/sbx/bin/bsdtar");
    assert_eq!(config.seed_tools_path().as_str(), "/sbx/bin/bsdtar");
    assert_eq!(config.seed_root().as_str(), "/skills");
    assert!(!config.store().path_style());
}

#[test]
fn the_required_keys_are_carried_as_given() {
    let config = config();
    assert_eq!(config.namespace(), "sbx-acme-none");
    assert_eq!(config.client_id().as_str(), "acme");
    assert_eq!(config.pool_id().as_str(), "main");
    assert_eq!(config.pool_id().replica_set_name(), "warm-main");
    assert_eq!(config.save_ttl(), Duration::from_secs(3600));
    assert_eq!(
        config.store().endpoint().as_str(),
        "https://store.example.test/"
    );
    assert_eq!(config.store().bucket(), "saves");
    assert_eq!(config.store().region(), "eu-west-1");
    assert_eq!(config.store().credentials().access_key_id(), "AKIDEXAMPLE");
}

#[test]
fn a_key_with_no_default_is_required_and_no_default_names_a_project_a_client_a_host_a_bucket_or_a_registry()
 {
    for key in [
        "namespace",
        "client_id",
        "pool_id",
        "network_mode",
        "save_ttl_seconds",
        "store_endpoint",
        "store_bucket",
        "store_region",
        "store_credentials",
    ] {
        assert!(input(without(key)).is_err(), "{key} is required");
    }
}

#[test]
fn the_names_of_the_keys_are_exact() {
    let mut wrong = without("namespace");
    wrong["Namespace"] = json!("sbx");
    assert!(input(wrong).is_err());
}

#[test]
fn a_key_that_the_table_does_not_name_is_not_read() {
    let built = build(with(json!({
        "image": "registry.example.test/agent:1",
        "cpu": "2",
        "proxy": "http://proxy.example.test",
    })))
    .expect("unknown keys are ignored");
    assert_eq!(built, config());
}

#[test]
fn the_network_modes_are_none_full_and_allow() {
    assert_eq!(
        build(with(json!({"network_mode": "full"})))
            .expect("full")
            .network()
            .mode(),
        NetworkMode::Full
    );
    let allow = build(with(json!({"network_mode": "allow", "allowlist": ["a.example.test", "b.example.test", "a.example.test"]})))
        .expect("allow");
    assert_eq!(allow.network().mode(), NetworkMode::Allow);
    assert!(matches!(allow.network(), NetworkRule::Allow(sites) if sites.len() == 2));
    assert!(input(with(json!({"network_mode": "open"}))).is_err());
    assert_eq!(required()["network_mode"], "none");
}

#[test]
fn the_optional_timeouts_and_the_other_keys_take_the_given_values() {
    let built = build(with(json!({
        "kube_context": "ctx",
        "label_prefix": "acme.io",
        "exec_timeout_seconds": 90,
        "save_timeout_seconds": 60,
        "save_zstd_level": 19,
        "restore_timeout_seconds": 10,
        "open_timeout_seconds": 5,
        "idle_ttl_seconds": 0,
        "idle_keep_min_state_bytes": 1,
        "claim_max_attempts": 1,
        "cold_create": false,
        "agent_container": "agent",
        "seed_container": "seeder",
        "tools_path": "/bin/bsdtar",
        "seed_tools_path": "/seed/bsdtar",
        "seed_root": "/seed",
        "store_path_style": true,
    })))
    .expect("every key set");
    assert_eq!(built.kube_context(), Some("ctx"));
    assert_eq!(built.label_prefix().as_str(), "acme.io");
    assert_eq!(built.exec_timeout(), Some(Duration::from_secs(90)));
    assert_eq!(built.save_timeout(), Some(Duration::from_secs(60)));
    assert_eq!(built.save_zstd_level(), 19);
    assert_eq!(built.restore_timeout(), Duration::from_secs(10));
    assert_eq!(built.open_timeout(), Duration::from_secs(5));
    assert_eq!(built.idle_ttl(), Duration::ZERO);
    assert_eq!(built.idle_keep_min_state_bytes(), 1);
    assert_eq!(built.claim_max_attempts(), 1);
    assert!(!built.cold_create());
    assert_eq!(built.agent_container(), "agent");
    assert_eq!(built.seed_container(), "seeder");
    assert_eq!(built.tools_path().as_str(), "/bin/bsdtar");
    assert_eq!(built.seed_tools_path().as_str(), "/seed/bsdtar");
    assert_eq!(built.seed_root().as_str(), "/seed");
    assert!(built.store().path_style());
}
