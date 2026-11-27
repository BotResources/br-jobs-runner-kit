use std::path::PathBuf;
use std::time::Duration;

use br_sandbox_keeper::KeeperConfig;
use serde_json::Value;

mod common;
mod keeper_config_map;

use common::{chart, rendered};
use keeper_config_map::keeper_document;

const SANDBOX_NAMESPACES: [&str; 4] = [
    "sbx-alpha-allow",
    "sbx-alpha-full",
    "sbx-alpha-none",
    "sbx-beta-none",
];

fn parsed(path: PathBuf) -> Value {
    let text = std::fs::read_to_string(&path).expect("a readable values file");
    serde_saphyr::from_str(&text).expect("a YAML values file")
}

fn overlay() -> Value {
    parsed(chart().join("ci/operator-nondefault-values.yaml"))
}

fn lookup<'a>(values: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(values, |value, key| &value[key])
}

fn number(values: &Value, path: &str) -> u64 {
    lookup(values, path)
        .as_u64()
        .unwrap_or_else(|| panic!("{path} is a number"))
}

fn text<'a>(values: &'a Value, path: &str) -> &'a str {
    lookup(values, path)
        .as_str()
        .unwrap_or_else(|| panic!("{path} is a string"))
}

fn seconds(values: &Value, path: &str) -> Duration {
    Duration::from_secs(number(values, path))
}

fn differs_from_the_defaults(overlay: &Value, path: &str) {
    for file in ["values.yaml", "ci/example-values.yaml"] {
        let base = parsed(chart().join(file));
        assert_ne!(
            lookup(overlay, path),
            lookup(&base, path),
            "{path} equals its value in {file}"
        );
    }
}

fn adapter_documents(objects: &[Value]) -> Vec<(String, Value)> {
    objects
        .iter()
        .filter(|object| {
            object["kind"] == "ConfigMap" && object["metadata"]["name"] == "br-sandbox-config"
        })
        .map(|object| {
            let namespace = object["metadata"]["namespace"]
                .as_str()
                .expect("a namespace");
            let document = object["data"]["config.json"]
                .as_str()
                .expect("a config.json key");
            (
                namespace.to_owned(),
                serde_json::from_str(document).expect("the adapter document is JSON"),
            )
        })
        .collect()
}

#[test]
fn the_values_of_the_overlay_are_not_the_defaults() {
    let overlay = overlay();
    for path in [
        "idle_ttl",
        "idle_keep_min_state_bytes",
        "warm_max_age_seconds",
        "save.ttl_seconds",
        "save.tombstone_ttl_days",
        "store.endpoint",
        "store.bucket",
        "store.region",
        "store.path_style",
        "operator.port",
        "operator.lease.duration_seconds",
        "operator.lease.renew_deadline_seconds",
        "operator.lease.retry_period_seconds",
    ] {
        differs_from_the_defaults(&overlay, path);
    }
}

#[test]
fn the_typed_config_carries_the_values_of_the_overlay() {
    let overlay = overlay();
    let objects = rendered(&chart().join("ci/operator-nondefault-values.yaml"), &[]);
    let config = KeeperConfig::from_json(&keeper_document(&objects)).expect("a valid document");

    assert_eq!(config.store.bucket, text(&overlay, "store.bucket"));
    assert_eq!(config.store.region, text(&overlay, "store.region"));
    assert_eq!(
        config.store.endpoint.as_str().trim_end_matches('/'),
        text(&overlay, "store.endpoint")
    );
    assert_eq!(
        config.store.path_style,
        lookup(&overlay, "store.path_style")
            .as_bool()
            .expect("a bool")
    );
    assert_eq!(config.save_ttl, seconds(&overlay, "save.ttl_seconds"));
    assert_eq!(
        config.tombstone_ttl,
        Duration::from_secs(number(&overlay, "save.tombstone_ttl_days") * 86_400)
    );
    assert_eq!(
        config.warm_max_age,
        seconds(&overlay, "warm_max_age_seconds")
    );
    assert_eq!(
        config.lease.duration,
        seconds(&overlay, "operator.lease.duration_seconds")
    );
    assert_eq!(
        config.lease.renew_deadline,
        seconds(&overlay, "operator.lease.renew_deadline_seconds")
    );
    assert_eq!(
        config.lease.retry_period,
        seconds(&overlay, "operator.lease.retry_period_seconds")
    );
    assert_eq!(
        config.listen_address.to_string(),
        format!("0.0.0.0:{}", number(&overlay, "operator.port"))
    );

    assert_eq!(config.idle_ttl, seconds(&overlay, "idle_ttl"));
    assert_eq!(
        config.idle_keep_min_state_bytes,
        number(&overlay, "idle_keep_min_state_bytes")
    );
}

#[test]
fn the_adapter_document_carries_the_same_values_as_the_keeper_document() {
    let objects = rendered(&chart().join("ci/operator-nondefault-values.yaml"), &[]);
    let config = KeeperConfig::from_json(&keeper_document(&objects)).expect("a valid document");
    let adapters = adapter_documents(&objects);
    assert_eq!(adapters.len(), SANDBOX_NAMESPACES.len());

    for (namespace, document) in &adapters {
        assert!(
            SANDBOX_NAMESPACES.contains(&namespace.as_str()),
            "the namespace {namespace}"
        );
        assert_eq!(text(document, "namespace"), namespace);
        assert_eq!(text(document, "pool_id"), namespace);
        assert_eq!(
            seconds(document, "idle_ttl_seconds"),
            config.idle_ttl,
            "{namespace}"
        );
        assert_eq!(
            number(document, "idle_keep_min_state_bytes"),
            config.idle_keep_min_state_bytes,
            "{namespace}"
        );
        assert_eq!(
            seconds(document, "save_ttl_seconds"),
            config.save_ttl,
            "{namespace}"
        );
        assert_eq!(
            text(document, "store_bucket"),
            config.store.bucket,
            "{namespace}"
        );
        assert_eq!(
            text(document, "store_region"),
            config.store.region,
            "{namespace}"
        );
        assert_eq!(
            text(document, "store_endpoint"),
            config.store.endpoint.as_str().trim_end_matches('/'),
            "{namespace}"
        );
        assert_eq!(
            lookup(document, "store_path_style").as_bool(),
            Some(config.store.path_style),
            "{namespace}"
        );
        for absent in [
            "floor",
            "max_sandboxes",
            "refill_seconds",
            "margin",
            "warm_max_age_seconds",
            "tombstone_ttl_seconds",
            "pool",
            "save",
        ] {
            assert!(
                document.get(absent).is_none(),
                "{namespace}: the adapter document holds {absent}"
            );
        }
    }
}
