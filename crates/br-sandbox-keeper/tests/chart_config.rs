use br_sandbox_keeper::{ConfigError, KeeperConfig};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod common;

use common::{chart, keeper_document, rendered, template};

fn overlays() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(chart().join("ci"))
        .expect("the chart has a ci folder")
        .map(|entry| entry.expect("a readable entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("-values.yaml") && name != "example-values.yaml")
        })
        .collect();
    found.sort();
    found
}

fn enables_the_keeper(overlay: &Path) -> bool {
    let text = std::fs::read_to_string(overlay).expect("a readable values file");
    let values: Value = serde_saphyr::from_str(&text).expect("a YAML values file");
    values["keeper"]["enabled"] == true
}

fn keeper_overlay() -> PathBuf {
    chart().join("ci/keeper-values.yaml")
}

fn document_of(objects: &[Value]) -> Value {
    serde_json::from_str(&keeper_document(objects)).expect("the keeper document is JSON")
}

fn enabling_overlays() -> Vec<PathBuf> {
    let enabling: Vec<PathBuf> = overlays()
        .into_iter()
        .filter(|overlay| enables_the_keeper(overlay))
        .collect();
    assert!(
        !enabling.is_empty(),
        "no values file of the chart CI enables the keeper"
    );
    enabling
}

fn fixture_of_chart_0_4() -> Value {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/chart-0.4-keeper-document.json");
    let text = std::fs::read_to_string(path).expect("the fixture of the chart 0.4 document");
    serde_json::from_str(&text).expect("the fixture is JSON")
}

#[test]
fn the_keeper_0_2_loads_a_document_of_the_shape_that_the_chart_0_4_renders() {
    let document = fixture_of_chart_0_4();
    assert!(document.get("pools").is_none());
    let config = KeeperConfig::from_json(&document.to_string())
        .unwrap_or_else(|error| panic!("the keeper refuses the fixture: {error}"));
    assert_eq!(config.agent_container, "agent");
    assert_eq!(config.managed_namespace_selector, "br.sandbox/managed=true");
    assert_eq!(config.store.bucket, "saves");
    assert_eq!(config.save_ttl, Duration::from_secs(60));
    assert_eq!(config.warm_max_age, Duration::from_secs(1800));
    assert_eq!(config.lease.duration, Duration::from_secs(15));
    assert_eq!(config.idle_ttl, Duration::from_secs(120));
    assert_eq!(config.idle_keep_min_state_bytes, 10_000_000);
    assert_eq!(config.listen_address.to_string(), "0.0.0.0:8080");
}

#[test]
fn the_keeper_0_2_refuses_the_pools_of_the_document_that_the_chart_0_3_renders() {
    for overlay in enabling_overlays() {
        let document = document_of(&rendered(&overlay, &[]));
        assert!(
            document["pools"]
                .as_array()
                .is_some_and(|pools| !pools.is_empty()),
            "{}",
            overlay.display()
        );
        assert_eq!(
            KeeperConfig::from_json(&document.to_string()),
            Err(ConfigError::PoolsAreResources),
            "{}",
            overlay.display()
        );
    }
}

#[test]
fn the_keeper_0_2_refuses_a_document_of_the_chart_0_4_shape_that_holds_the_pools() {
    let mut document = fixture_of_chart_0_4();
    document["pools"] = Value::Array(Vec::new());
    assert_eq!(
        KeeperConfig::from_json(&document.to_string()),
        Err(ConfigError::PoolsAreResources)
    );
}

#[test]
fn the_document_of_the_chart_carries_the_store_the_lease_and_the_settings_of_the_values() {
    let document = document_of(&rendered(&keeper_overlay(), &[]));
    assert_eq!(document["store_endpoint"], "https://store.example.test");
    assert_eq!(document["store_region"], "example-1");
    assert_eq!(document["store_path_style"], true);
    assert_eq!(document["save_ttl_seconds"], 60);
    assert_eq!(document["tombstone_ttl_seconds"], 365 * 86_400);
    assert_eq!(document["warm_max_age_seconds"], 1800);
    assert_eq!(document["lease_duration_seconds"], 15);
    assert_eq!(document["renew_deadline_seconds"], 10);
    assert_eq!(document["retry_period_seconds"], 2);
    assert!(document.get("lease_namespace").is_none());
    assert_eq!(document["listen_address"], "0.0.0.0:8080");
    assert_eq!(
        document["managed_namespace_selector"],
        "br.sandbox/managed=true"
    );
    assert_eq!(document["scale_down_cooldown_seconds"], 120);
}

#[test]
fn the_port_of_the_value_is_the_listen_address_and_the_container_port() {
    let objects = rendered(&keeper_overlay(), &["--set", "keeper.port=9100"]);
    assert_eq!(document_of(&objects)["listen_address"], "0.0.0.0:9100");
    let deployment = objects
        .iter()
        .find(|object| {
            object["kind"] == "Deployment" && object["metadata"]["name"] == "br-sandbox-keeper"
        })
        .expect("the keeper Deployment");
    let container = &deployment["spec"]["template"]["spec"]["containers"][0];
    assert_eq!(container["ports"][0]["containerPort"], 9100);
    assert_eq!(container["livenessProbe"]["httpGet"]["path"], "/livez");
    assert_eq!(container["readinessProbe"]["httpGet"]["path"], "/readyz");
}

#[test]
fn a_settings_key_that_the_chart_computes_fails_the_render() {
    for key in [
        "save_ttl_seconds",
        "store_bucket",
        "pools",
        "listen_address",
        "lease_duration_seconds",
    ] {
        let setting = format!("keeper.settings.{key}=1");
        let output = template(&keeper_overlay(), &["--set", &setting]);
        let errors = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{key}: the render passed");
        assert!(
            errors.contains(&format!("BR_SANDBOX_KEEPER_SETTING_COMPUTED{{key={key}}}")),
            "{key}: {errors}"
        );
    }
}

#[test]
fn a_settings_key_that_the_keeper_does_not_know_reaches_the_keeper_and_fails_there() {
    let document = document_of(&rendered(
        &keeper_overlay(),
        &["--set", "keeper.settings.not_a_key=1"],
    ));
    let error = KeeperConfig::from_json(&document.to_string())
        .expect_err("the keeper refuses an unknown key");
    assert!(error.to_string().contains("not_a_key"), "{error}");
}
