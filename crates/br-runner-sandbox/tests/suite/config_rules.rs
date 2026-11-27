use br_runner_sandbox::ConfigError;
use serde_json::{Value, json};

use crate::common::{build, with};

fn refused(extra: Value) -> ConfigError {
    build(with(extra)).expect_err("the value breaks a rule")
}

fn with_user_information(user: &str, password: &str, host: &str) -> String {
    format!("https://{user}:{password}@{host}")
}

#[test]
fn a_blank_text_is_refused_for_each_key_that_must_not_be_blank() {
    for (key, error_key) in [
        ("namespace", "namespace"),
        ("client_id", "client_id"),
        ("label_prefix", "label_prefix"),
        ("pool_id", "pool_id"),
        ("kube_context", "kube_context"),
        ("store_bucket", "store_bucket"),
        ("store_region", "store_region"),
        ("agent_container", "agent_container"),
        ("seed_container", "seed_container"),
    ] {
        assert_eq!(
            refused(json!({ key: "  " })),
            ConfigError::Blank { key: error_key }
        );
    }
}

#[test]
fn the_client_and_the_label_prefix_hold_no_slash() {
    assert_eq!(
        refused(json!({"client_id": "a/b"})),
        ConfigError::HoldsSlash { key: "client_id" }
    );
    assert_eq!(
        refused(json!({"label_prefix": "br/sandbox"})),
        ConfigError::HoldsSlash {
            key: "label_prefix"
        }
    );
}

#[test]
fn the_allowlist_is_not_empty_for_allow_and_empty_for_none_and_full() {
    assert_eq!(
        refused(json!({"network_mode": "allow"})),
        ConfigError::AllowlistRequired
    );
    assert_eq!(
        refused(json!({"network_mode": "none", "allowlist": ["a.example.test"]})),
        ConfigError::AllowlistForbidden { mode: "none" }
    );
    assert_eq!(
        refused(json!({"network_mode": "full", "allowlist": ["a.example.test"]})),
        ConfigError::AllowlistForbidden { mode: "full" }
    );
    assert_eq!(
        refused(json!({"network_mode": "allow", "allowlist": [" "]})),
        ConfigError::BlankSite
    );
}

#[test]
fn the_durations_that_must_be_above_zero_are_refused_at_zero() {
    for key in [
        "save_ttl_seconds",
        "restore_timeout_seconds",
        "open_timeout_seconds",
        "exec_timeout_seconds",
        "save_timeout_seconds",
    ] {
        assert_eq!(
            refused(json!({ key: 0 })),
            ConfigError::NotAboveZero { key }
        );
    }
}

#[test]
fn a_negative_integer_is_not_a_duration() {
    assert!(crate::common::input(with(json!({"save_ttl_seconds": -1}))).is_err());
}

#[test]
fn the_zstd_level_is_one_to_twenty_two() {
    for level in [0, 23, -1] {
        assert_eq!(
            refused(json!({"save_zstd_level": level})),
            ConfigError::ZstdLevel { level }
        );
    }
    for level in [1, 22] {
        assert!(build(with(json!({"save_zstd_level": level}))).is_ok());
    }
}

#[test]
fn the_claim_tries_are_at_least_one() {
    assert_eq!(
        refused(json!({"claim_max_attempts": 0})),
        ConfigError::ClaimAttempts
    );
}

#[test]
fn the_save_scope_holds_work() {
    assert_eq!(
        refused(json!({"save_scope_paths": ["/home/agent"]})),
        ConfigError::ScopeWithoutWork
    );
    assert_eq!(
        refused(json!({"save_scope_paths": ["/"]})),
        ConfigError::ScopeWithoutWork
    );
    assert!(build(with(json!({"save_scope_paths": ["/work"]}))).is_ok());
}

#[test]
fn a_save_scope_path_is_absolute_and_has_no_parent_component() {
    assert!(matches!(
        refused(json!({"save_scope_paths": ["/work", "home/agent"]})),
        ConfigError::NotAbsolute { .. }
    ));
    assert!(matches!(
        refused(json!({"save_scope_paths": ["/work", "/home/../etc"]})),
        ConfigError::PathEscapes { .. }
    ));
}

#[test]
fn no_save_scope_path_holds_another() {
    for scope in [
        json!(["/work", "/work/project"]),
        json!(["/work/project", "/work"]),
        json!(["/work", "/work"]),
        json!(["/work", "/work/"]),
        json!(["/", "/work"]),
    ] {
        assert!(matches!(
            refused(json!({"save_scope_paths": scope})),
            ConfigError::ScopeNested { .. }
        ));
    }
    assert!(build(with(json!({"save_scope_paths": ["/work", "/workspace"]}))).is_ok());
}

#[test]
fn the_seed_root_is_outside_the_save_scope() {
    for seed_root in ["/work", "/work/skills", "/home", "/", "/home/agent/skills"] {
        assert!(
            matches!(
                refused(json!({"seed_root": seed_root})),
                ConfigError::SeedRootInScope { .. }
            ),
            "{seed_root}"
        );
    }
    assert!(build(with(json!({"seed_root": "/workspace"}))).is_ok());
}

#[test]
fn the_tool_paths_and_the_seed_root_are_absolute() {
    for key in ["tools_path", "seed_tools_path", "seed_root"] {
        assert!(matches!(
            refused(json!({ key: "relative/bsdtar" })),
            ConfigError::NotAbsolute { .. }
        ));
    }
}

#[test]
fn the_store_endpoint_is_an_http_url_without_credentials() {
    for endpoint in [
        "store.example.test",
        "ftp://store.example.test",
        &with_user_information("key", "secret", "store.example.test"),
        "https://key@store.example.test",
        "",
    ] {
        assert_eq!(
            refused(json!({"store_endpoint": endpoint})),
            ConfigError::StoreEndpoint,
            "{endpoint:?}"
        );
    }
    assert!(build(with(json!({"store_endpoint": "http://127.0.0.1:9000"}))).is_ok());
}

#[test]
fn a_blank_store_credential_is_refused() {
    for credentials in [
        json!({"access_key_id": "", "secret_access_key": "s"}),
        json!({"access_key_id": "k", "secret_access_key": " "}),
    ] {
        assert!(crate::common::input(with(json!({"store_credentials": credentials}))).is_err());
    }
}

#[test]
fn the_error_of_a_rule_holds_no_credential() {
    let endpoint = with_user_information("AKIDEXAMPLE", "wJalrSECRETEXAMPLE", "h.example.test");
    let error = refused(json!({ "store_endpoint": endpoint }));
    let text = format!("{error} {error:?}");
    assert!(!text.contains("AKIDEXAMPLE") && !text.contains("wJalrSECRETEXAMPLE"));
}
