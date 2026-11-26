use std::time::Duration;

use serde_json::{Value, json};

use super::*;

fn document() -> Value {
    json!({
        "store_bucket": "saves",
        "store_endpoint": "https://s3.example.test",
        "store_region": "eu",
        "listen_address": "0.0.0.0:8080",
        "save_ttl_seconds": 604800,
        "pools": [{
            "id": "acme-none",
            "namespace": "sbx-acme-none",
            "replicaset": "warm-acme-none",
            "min_replicas": 1,
            "max_replicas": 20
        }]
    })
}

fn load(document: &Value) -> Result<KeeperConfig, ConfigError> {
    KeeperConfig::from_json(&document.to_string())
}

fn with(key: &str, value: Value) -> Value {
    let mut base = document();
    base[key] = value;
    base
}

fn with_pool(key: &str, value: Value) -> Value {
    let mut base = document();
    base["pools"][0][key] = value;
    base
}

fn without(key: &str) -> Value {
    let mut base = document();
    base.as_object_mut().unwrap().remove(key);
    base
}

#[test]
fn a_minimal_document_takes_every_default_of_the_table() {
    let config = load(&document()).unwrap();
    assert_eq!(config.label_prefix.as_str(), "br.sandbox");
    assert_eq!(config.managed_namespace_selector, "br.sandbox/managed=true");
    assert!(!config.store.path_style);
    assert_eq!(config.skew_margin, Duration::from_secs(60));
    assert_eq!(config.sweep_interval, Duration::from_secs(60));
    assert_eq!(config.sweep_page_size, 1000);
    assert_eq!(config.attempt_ttl, Duration::from_secs(3600));
    assert_eq!(config.tombstone_ttl, Duration::from_secs(31_536_000));
    assert_eq!(config.stuck_ttl, Duration::from_secs(7200));
    assert_eq!(config.failed_pod_ttl, Duration::from_secs(300));
    assert_eq!(config.warm_max_age, Duration::from_secs(1800));
    assert_eq!(config.pending_alarm, Duration::from_secs(60));
    assert_eq!(config.rate_window, Duration::from_secs(300));
    assert_eq!(config.rate_bucket, Duration::from_secs(10));
    assert_eq!(config.scale_down_cooldown, Duration::from_secs(120));
    assert_eq!(config.scale_down_step, 5);
    assert_eq!(config.scale_down_step_interval, Duration::from_secs(10));
    assert_eq!(config.lease.namespace, None);
    assert_eq!(config.lease.duration, Duration::from_secs(15));
    assert_eq!(config.lease.renew_deadline, Duration::from_secs(10));
    assert_eq!(config.lease.retry_period, Duration::from_secs(2));
    assert_eq!(config.timezone, chrono_tz::Tz::UTC);
    let pool = &config.pools[0];
    assert_eq!(pool.refill, Duration::from_secs(3));
    assert_eq!(pool.margin, 2);
    assert!(pool.floors.is_empty());
    assert_eq!(pool.idle_ttl, Duration::from_secs(120));
    assert_eq!(pool.idle_keep_min_state_bytes, 10_000_000);
}

#[test]
fn the_default_selector_follows_the_label_prefix() {
    let config = load(&with("label_prefix", json!("acme.sbx"))).unwrap();
    assert_eq!(config.managed_namespace_selector, "acme.sbx/managed=true");
    let config = load(&with("managed_namespace_selector", json!("team=a"))).unwrap();
    assert_eq!(config.managed_namespace_selector, "team=a");
}

#[test]
fn a_key_that_the_library_does_not_know_is_a_fault() {
    let result = load(&with("store_credentials_secret", json!("x")));
    assert!(
        matches!(result, Err(ConfigError::Document(text)) if text.contains("store_credentials_secret"))
    );
    let result = load(&with_pool("size", json!(1)));
    assert!(matches!(result, Err(ConfigError::Document(text)) if text.contains("size")));
}

#[test]
fn a_document_that_is_not_json_is_a_fault() {
    assert!(matches!(
        KeeperConfig::from_json("store_bucket: x"),
        Err(ConfigError::Document(_))
    ));
}

#[test]
fn each_required_key_is_required() {
    for key in [
        "store_bucket",
        "store_endpoint",
        "store_region",
        "listen_address",
        "save_ttl_seconds",
        "pools",
    ] {
        assert_eq!(
            load(&without(key)),
            Err(ConfigError::Missing {
                key: key.to_owned()
            }),
            "{key}"
        );
    }
    for key in [
        "id",
        "namespace",
        "replicaset",
        "min_replicas",
        "max_replicas",
    ] {
        let mut base = document();
        base["pools"][0].as_object_mut().unwrap().remove(key);
        assert_eq!(
            load(&base),
            Err(ConfigError::Missing {
                key: format!("pools[0].{key}")
            }),
            "{key}"
        );
    }
}

#[test]
fn a_save_age_of_zero_or_below_does_not_start_the_process() {
    for value in [0, -1] {
        assert_eq!(
            load(&with("save_ttl_seconds", json!(value))),
            Err(ConfigError::NotAboveZero {
                key: "save_ttl_seconds".to_owned()
            })
        );
    }
}

#[test]
fn a_duration_that_drives_a_timer_is_never_zero() {
    for key in [
        "sweep_interval_seconds",
        "attempt_ttl_seconds",
        "tombstone_ttl_seconds",
        "stuck_ttl_seconds",
        "failed_pod_ttl_seconds",
        "warm_max_age_seconds",
        "pending_alarm_seconds",
        "rate_window_seconds",
        "rate_bucket_seconds",
        "scale_down_cooldown_seconds",
        "scale_down_step_interval_seconds",
        "lease_duration_seconds",
        "renew_deadline_seconds",
        "retry_period_seconds",
        "sweep_page_size",
    ] {
        assert_eq!(
            load(&with(key, json!(0))),
            Err(ConfigError::NotAboveZero {
                key: key.to_owned()
            }),
            "{key}"
        );
    }
}

#[test]
fn the_idle_ttl_and_the_skew_margin_may_be_zero_but_not_negative() {
    let config = load(&with("skew_margin_seconds", json!(0))).unwrap();
    assert_eq!(config.skew_margin, Duration::ZERO);
    let config = load(&with_pool("idle_ttl_seconds", json!(0))).unwrap();
    assert_eq!(config.pools[0].idle_ttl, Duration::ZERO);
    assert_eq!(
        load(&with("skew_margin_seconds", json!(-1))),
        Err(ConfigError::Negative {
            key: "skew_margin_seconds".to_owned()
        })
    );
    assert_eq!(
        load(&with_pool("idle_ttl_seconds", json!(-1))),
        Err(ConfigError::Negative {
            key: "pools[0].idle_ttl_seconds".to_owned()
        })
    );
}

#[test]
fn min_replicas_above_max_replicas_is_a_fault() {
    let mut base = with_pool("min_replicas", json!(21));
    base["pools"][0]["max_replicas"] = json!(20);
    assert_eq!(
        load(&base),
        Err(ConfigError::MinAboveMax {
            pool: "acme-none".to_owned()
        })
    );
    let mut base = with_pool("min_replicas", json!(20));
    base["pools"][0]["max_replicas"] = json!(20);
    assert!(load(&base).is_ok());
}

#[test]
fn a_rate_bucket_above_the_rate_window_is_a_fault() {
    let mut base = with("rate_bucket_seconds", json!(301));
    base["rate_window_seconds"] = json!(300);
    assert_eq!(load(&base), Err(ConfigError::BucketAboveWindow));
    base["rate_bucket_seconds"] = json!(300);
    assert!(load(&base).is_ok());
}

#[test]
fn a_scale_down_step_below_one_is_a_fault() {
    assert_eq!(
        load(&with("scale_down_step", json!(0))),
        Err(ConfigError::StepBelowOne)
    );
    assert!(load(&with("scale_down_step", json!(1))).is_ok());
}

#[test]
fn the_lease_durations_must_fall_in_strict_order() {
    for (duration, renew, retry) in [(10, 10, 2), (15, 10, 10), (5, 10, 2), (15, 2, 10)] {
        let mut base = with("lease_duration_seconds", json!(duration));
        base["renew_deadline_seconds"] = json!(renew);
        base["retry_period_seconds"] = json!(retry);
        assert_eq!(
            load(&base),
            Err(ConfigError::LeaseOrder),
            "{duration} {renew} {retry}"
        );
    }
}

#[test]
fn the_time_zone_must_be_a_known_name() {
    assert!(load(&with("timezone", json!("Europe/Paris"))).is_ok());
    assert_eq!(
        load(&with("timezone", json!("Mars/Olympus"))),
        Err(ConfigError::UnknownTimezone("Mars/Olympus".to_owned()))
    );
}

#[test]
fn a_floor_window_that_ends_no_later_than_it_begins_is_a_fault() {
    for (from, to) in [("09:00", "09:00"), ("22:00", "02:00"), ("10:00", "09:59")] {
        let result = load(&with_pool(
            "floors",
            json!([{"from": from, "to": to, "floor": 5}]),
        ));
        assert_eq!(
            result,
            Err(ConfigError::WindowEndsNoLater {
                pool: "acme-none".to_owned(),
                index: 0
            }),
            "{from} {to}"
        );
    }
}

#[test]
fn a_window_that_ends_at_midnight_ends_at_24_00() {
    let config = load(&with_pool(
        "floors",
        json!([{"from": "22:00", "to": "24:00", "floor": 5}, {"from": "00:00", "to": "02:00", "floor": 5}]),
    ))
    .unwrap();
    assert_eq!(config.pools[0].floors[0].to_second, 86_400);
    assert_eq!(config.pools[0].floors[1].from_second, 0);
}

#[test]
fn a_time_of_day_that_is_not_a_time_is_a_fault() {
    for text in ["9:00", "25:00", "10:61", "10", "ten", "24:00:01"] {
        let result = load(&with_pool(
            "floors",
            json!([{"from": text, "to": "24:00", "floor": 5}]),
        ));
        assert!(matches!(result, Err(ConfigError::Invalid { .. })), "{text}");
    }
    let result = load(&with_pool(
        "floors",
        json!([{"from": "24:00", "to": "24:00", "floor": 5}]),
    ));
    assert!(matches!(result, Err(ConfigError::Invalid { .. })));
}

#[test]
fn two_pools_with_one_id_or_one_replica_set_in_one_namespace_are_a_fault() {
    let mut base = document();
    let mut second = base["pools"][0].clone();
    second["replicaset"] = json!("warm-other");
    base["pools"].as_array_mut().unwrap().push(second);
    assert_eq!(
        load(&base),
        Err(ConfigError::DuplicatePoolId("acme-none".to_owned()))
    );
    let mut base = document();
    let mut second = base["pools"][0].clone();
    second["id"] = json!("other");
    base["pools"].as_array_mut().unwrap().push(second);
    assert_eq!(
        load(&base),
        Err(ConfigError::DuplicateReplicaSet {
            namespace: "sbx-acme-none".to_owned(),
            replicaset: "warm-acme-none".to_owned()
        })
    );
    let mut second = base["pools"][1].clone();
    second["namespace"] = json!("sbx-other");
    base["pools"][1] = second;
    assert!(load(&base).is_ok());
}

#[test]
fn a_document_with_no_pool_is_a_fault() {
    assert_eq!(load(&with("pools", json!([]))), Err(ConfigError::NoPools));
}

#[test]
fn the_store_endpoint_is_an_http_url_with_no_user_information() {
    assert!(load(&with("store_endpoint", json!("http://seaweed:8333"))).is_ok());
    for bad in [
        "s3.example.test",
        "ftp://host",
        "https://user:pw@host",
        "file:///x",
    ] {
        assert!(
            matches!(
                load(&with("store_endpoint", json!(bad))),
                Err(ConfigError::Invalid { .. })
            ),
            "{bad}"
        );
    }
}

#[test]
fn the_listen_address_is_a_socket_address() {
    assert!(matches!(
        load(&with("listen_address", json!("localhost"))),
        Err(ConfigError::Invalid { .. })
    ));
}

#[test]
fn a_blank_text_value_is_a_fault() {
    assert!(matches!(
        load(&with("store_bucket", json!("  "))),
        Err(ConfigError::Invalid { .. })
    ));
}

#[test]
fn the_pool_lookup_needs_the_namespace_and_the_id() {
    let config = load(&document()).unwrap();
    assert!(config.pool("sbx-acme-none", "acme-none").is_some());
    assert!(config.pool("sbx-other", "acme-none").is_none());
    assert!(config.pool("sbx-acme-none", "other").is_none());
}
