use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const KEEPER_CONFIG_MAP: &str = "br-sandbox-keeper-config";
const KEEPER_CONFIG_KEY: &str = "config.json";

pub fn chart() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../charts/br-sandbox-chart")
}

fn example() -> PathBuf {
    chart().join("ci/example-values.yaml")
}

fn helm(arguments: &[String]) -> std::process::Output {
    Command::new("helm")
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("helm must be installed to run this test: {error}"))
}

pub fn template(overlay: &Path, extra: &[&str]) -> std::process::Output {
    let mut arguments: Vec<String> = vec![
        "template".into(),
        "t".into(),
        chart().display().to_string(),
        "-f".into(),
        example().display().to_string(),
        "-f".into(),
        overlay.display().to_string(),
    ];
    arguments.extend(extra.iter().map(|argument| (*argument).to_owned()));
    helm(&arguments)
}

pub fn rendered(overlay: &Path, extra: &[&str]) -> Vec<Value> {
    let output = template(overlay, extra);
    assert!(
        output.status.success(),
        "helm template failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("helm prints UTF-8");
    serde_saphyr::from_multiple::<Value>(&text)
        .expect("helm prints YAML")
        .into_iter()
        .filter(|object| !object.is_null())
        .collect()
}

pub fn keeper_document(objects: &[Value]) -> String {
    let map = objects
        .iter()
        .find(|object| {
            object["kind"] == "ConfigMap" && object["metadata"]["name"] == KEEPER_CONFIG_MAP
        })
        .expect("the chart renders the keeper ConfigMap");
    map["data"][KEEPER_CONFIG_KEY]
        .as_str()
        .expect("the keeper ConfigMap holds the key config.json")
        .to_owned()
}
