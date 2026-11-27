use serde_json::Value;
use std::path::PathBuf;

mod common;

use common::{chart, keeper_document, rendered};

const SANDBOX_NAMESPACES: [(&str, &str, &str); 4] = [
    ("sbx-alpha-allow", "alpha-allow", "pool"),
    (
        "sbx-alpha-full",
        "alpha-full",
        "clients.alpha.modes.full.pool",
    ),
    ("sbx-alpha-none", "alpha-none", "pool"),
    ("sbx-beta-none", "beta-none", "pool"),
];

fn parsed(path: PathBuf) -> Value {
    let text = std::fs::read_to_string(&path).expect("a readable values file");
    serde_saphyr::from_str(&text).expect("a YAML values file")
}

fn overlay() -> Value {
    parsed(chart().join("ci/keeper-nondefault-values.yaml"))
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
        "pool.floor",
        "pool.max_sandboxes",
        "pool.refill_seconds",
        "pool.margin",
        "keeper.port",
        "keeper.lease.duration_seconds",
        "keeper.lease.renew_deadline_seconds",
        "keeper.lease.retry_period_seconds",
    ] {
        differs_from_the_defaults(&overlay, path);
    }
}

fn keeper_values() -> Value {
    let objects = rendered(&chart().join("ci/keeper-nondefault-values.yaml"), &[]);
    serde_json::from_str(&keeper_document(&objects)).expect("the keeper document is JSON")
}

#[test]
fn the_keeper_document_carries_the_values_of_the_overlay() {
    let overlay = overlay();
    let document = keeper_values();

    assert_eq!(
        text(&document, "store_bucket"),
        text(&overlay, "store.bucket")
    );
    assert_eq!(
        text(&document, "store_region"),
        text(&overlay, "store.region")
    );
    assert_eq!(
        text(&document, "store_endpoint"),
        text(&overlay, "store.endpoint")
    );
    assert_eq!(
        lookup(&document, "store_path_style").as_bool(),
        lookup(&overlay, "store.path_style").as_bool()
    );
    assert_eq!(
        number(&document, "save_ttl_seconds"),
        number(&overlay, "save.ttl_seconds")
    );
    assert_eq!(
        number(&document, "tombstone_ttl_seconds"),
        number(&overlay, "save.tombstone_ttl_days") * 86_400
    );
    assert_eq!(
        number(&document, "warm_max_age_seconds"),
        number(&overlay, "warm_max_age_seconds")
    );
    for (key, path) in [
        ("lease_duration_seconds", "keeper.lease.duration_seconds"),
        (
            "renew_deadline_seconds",
            "keeper.lease.renew_deadline_seconds",
        ),
        ("retry_period_seconds", "keeper.lease.retry_period_seconds"),
    ] {
        assert_eq!(number(&document, key), number(&overlay, path), "{key}");
    }
    assert_eq!(
        text(&document, "listen_address"),
        format!("0.0.0.0:{}", number(&overlay, "keeper.port"))
    );

    let pools = document["pools"]
        .as_array()
        .expect("the pools of the chart");
    assert_eq!(pools.len(), SANDBOX_NAMESPACES.len());
    for (namespace, id, source) in SANDBOX_NAMESPACES {
        let pool = pools
            .iter()
            .find(|pool| pool["id"] == id && pool["namespace"] == namespace)
            .unwrap_or_else(|| panic!("the pool {id}"));
        for (key, field) in [
            ("min_replicas", "floor"),
            ("max_replicas", "max_sandboxes"),
            ("refill_seconds", "refill_seconds"),
            ("margin", "margin"),
        ] {
            assert_eq!(
                number(pool, key),
                number(&overlay, &format!("{source}.{field}")),
                "{id} {key}"
            );
        }
        assert_eq!(
            number(pool, "idle_ttl_seconds"),
            number(&overlay, "idle_ttl"),
            "{id}"
        );
        assert_eq!(
            number(pool, "idle_keep_min_state_bytes"),
            number(&overlay, "idle_keep_min_state_bytes"),
            "{id}"
        );
    }
}

#[test]
fn the_adapter_document_carries_the_same_values_as_the_keeper_document() {
    let objects = rendered(&chart().join("ci/keeper-nondefault-values.yaml"), &[]);
    let keeper = keeper_values();
    let adapters = adapter_documents(&objects);
    assert_eq!(adapters.len(), SANDBOX_NAMESPACES.len());

    for (namespace, document) in &adapters {
        let (_, id, _) = SANDBOX_NAMESPACES
            .iter()
            .find(|(name, _, _)| name == namespace)
            .unwrap_or_else(|| panic!("the namespace {namespace}"));
        let pool = keeper["pools"]
            .as_array()
            .and_then(|pools| pools.iter().find(|pool| pool["namespace"] == *namespace))
            .expect("the pool of the namespace");
        assert_eq!(text(document, "namespace"), namespace);
        assert_eq!(text(document, "pool_id"), *id);
        assert_eq!(
            number(document, "idle_ttl_seconds"),
            number(pool, "idle_ttl_seconds"),
            "{namespace}"
        );
        assert_eq!(
            number(document, "idle_keep_min_state_bytes"),
            number(pool, "idle_keep_min_state_bytes"),
            "{namespace}"
        );
        for key in [
            "save_ttl_seconds",
            "store_bucket",
            "store_region",
            "store_endpoint",
            "store_path_style",
        ] {
            assert_eq!(document[key], keeper[key], "{namespace} {key}");
        }
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
