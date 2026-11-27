use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use br_runner_sandbox::{ConfigError, SandboxConfig, SandboxConfigInput};
use serde_json::{Map, Value, json};

const CONFIG_MAP: &str = "br-sandbox-config";
const CONFIG_KEY: &str = "config.json";
const KEY: &str = "exec_timeout_seconds";
const RENDER_FAULT: &str = "BR_SANDBOX_EXEC_TIMEOUT_INVALID";
const DEFAULT: u64 = 600;
const OVERLAY_VALUE: u64 = 1800;

fn chart() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../charts/br-sandbox-chart")
}

fn render(overlay: Option<&Path>, sets: &[&str]) -> std::process::Output {
    let example = chart().join("ci/example-values.yaml");
    let files: Vec<String> = std::iter::once(example)
        .chain(overlay.map(Path::to_path_buf))
        .flat_map(|file| ["-f".to_owned(), file.display().to_string()])
        .collect();
    Command::new("helm")
        .args(["template", "t"])
        .arg(chart())
        .args(&files)
        .args(sets.iter().flat_map(|set| ["--set", *set]))
        .output()
        .unwrap_or_else(|error| panic!("helm must be installed to run this test: {error}"))
}

fn documents(overlay: Option<&Path>, sets: &[&str]) -> Vec<(String, Map<String, Value>)> {
    let output = render(overlay, sets);
    assert!(
        output.status.success(),
        "helm template failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("helm prints UTF-8");
    let found: Vec<(String, Map<String, Value>)> = serde_saphyr::from_multiple::<Value>(&text)
        .expect("helm prints YAML")
        .iter()
        .filter(|object| object["kind"] == "ConfigMap" && object["metadata"]["name"] == CONFIG_MAP)
        .map(|map| {
            let namespace = map["metadata"]["namespace"].as_str().expect("a namespace");
            let document = map["data"][CONFIG_KEY]
                .as_str()
                .expect("the key config.json");
            match serde_json::from_str(document).expect("the document is JSON") {
                Value::Object(object) => (namespace.to_owned(), object),
                other => panic!("{namespace}: the document is not an object: {other}"),
            }
        })
        .collect();
    assert!(found.len() >= 4, "fewer than four sandbox namespaces");
    found
}

fn renders_no_namespace(path: &Path) -> bool {
    let text = std::fs::read_to_string(path).expect("a readable values file");
    let values: Value = serde_saphyr::from_str(&text).expect("a YAML values file");
    values
        .get("namespaces")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
}

fn fixtures() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(chart().join("ci"))
        .expect("the chart has a ci folder")
        .map(|entry| entry.expect("a readable entry").path())
        .filter(|path| {
            let name = path.file_name().and_then(|name| name.to_str());
            name.is_some_and(|name| name.ends_with("-values.yaml") && name != "example-values.yaml")
                && !renders_no_namespace(path)
        })
        .collect();
    found.sort();
    found
}

fn value_of_overlay(overlay: &Path) -> u64 {
    let text = std::fs::read_to_string(overlay).expect("a readable values file");
    let values: Value = serde_saphyr::from_str(&text).expect("a YAML values file");
    values.get(KEY).map_or(DEFAULT, |value| {
        value.as_u64().expect("the overlay value is an integer")
    })
}

fn with_an_allowlist_when_mode_allow(mut document: Map<String, Value>) -> Map<String, Value> {
    if document["network_mode"] == "allow" && !document.contains_key("allowlist") {
        document.insert("allowlist".into(), json!(["git.example.test"]));
    }
    document
}

fn input(document: &Map<String, Value>) -> SandboxConfigInput {
    let mut full = document.clone();
    full.insert(
        "store_credentials".into(),
        json!({"access_key_id": "test-chart-key-id", "secret_access_key": "test-chart-secret-value"}),
    );
    serde_json::from_value(Value::Object(full)).expect("the adapter reads the document")
}

fn held_by_the_adapter(document: &Map<String, Value>) -> Option<Duration> {
    let loaded = input(&with_an_allowlist_when_mode_allow(document.clone()));
    assert_eq!(loaded.exec_timeout_seconds, document[KEY].as_u64());
    SandboxConfig::try_from(loaded)
        .expect("the adapter accepts the document")
        .exec_timeout()
}

fn assert_every_namespace_holds(documents: &[(String, Map<String, Value>)], seconds: u64) {
    for (namespace, document) in documents {
        assert_eq!(document[KEY], seconds, "{namespace}: the rendered key");
        assert_eq!(
            held_by_the_adapter(document),
            Some(Duration::from_secs(seconds)),
            "{namespace}: the bound that the adapter holds"
        );
    }
}

#[test]
fn the_default_renders_600_and_the_adapter_holds_600() {
    assert_every_namespace_holds(&documents(None, &[]), DEFAULT);
}

#[test]
fn every_fixture_renders_the_key_and_the_adapter_holds_the_rendered_value() {
    let overlays = fixtures();
    assert!(
        overlays
            .iter()
            .any(|overlay| value_of_overlay(overlay) != DEFAULT),
        "no fixture sets the key away from its default"
    );
    for overlay in overlays {
        let expected = value_of_overlay(&overlay);
        assert_every_namespace_holds(&documents(Some(&overlay), &[]), expected);
    }
}

#[test]
fn the_overlay_that_sets_the_key_gives_1800_to_the_adapter() {
    let overlay = chart().join("ci/exec-timeout-values.yaml");
    assert_eq!(value_of_overlay(&overlay), OVERLAY_VALUE);
    assert_every_namespace_holds(&documents(Some(&overlay), &[]), OVERLAY_VALUE);
}

#[test]
fn a_value_set_on_the_command_line_reaches_the_adapter() {
    assert_every_namespace_holds(&documents(None, &["exec_timeout_seconds=45"]), 45);
}

#[test]
fn the_adapter_refuses_a_document_with_a_zero_bound_and_the_chart_never_renders_one() {
    let (_, mut document) = documents(None, &[]).remove(0);
    document.insert(KEY.into(), json!(0));
    let refused = SandboxConfig::try_from(input(&with_an_allowlist_when_mode_allow(document)));
    assert!(
        matches!(refused, Err(ConfigError::NotAboveZero { key }) if key == KEY),
        "{refused:?}"
    );
    for refused in ["exec_timeout_seconds=0", "exec_timeout_seconds=-1"] {
        let failure = failure_text(&[refused]);
        assert!(failure.contains(KEY), "{refused}: {failure}");
        assert!(!failure.contains(RENDER_FAULT), "{refused}: {failure}");
    }
    let failure = failure_text(&["exec_timeout_seconds=null"]);
    assert!(failure.contains(RENDER_FAULT), "null: {failure}");
}

fn failure_text(sets: &[&str]) -> String {
    let output = render(None, sets);
    assert!(!output.status.success(), "{sets:?} rendered");
    String::from_utf8_lossy(&output.stderr).into_owned()
}
