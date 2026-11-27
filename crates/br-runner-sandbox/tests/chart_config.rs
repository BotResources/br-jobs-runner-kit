use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use br_runner_sandbox::{ConfigError, NetworkRule, SandboxConfig, SandboxConfigInput};
use serde_json::{Map, Value, json};

const CONFIG_MAP: &str = "br-sandbox-config";
const CONFIG_KEY: &str = "config.json";
const ACCESS_KEY: &str = "test-chart-key-id";
const SECRET_KEY: &str = "test-chart-secret-value";

const CHANGED: [&str; 8] = [
    "restore_timeout_seconds=45",
    "idle_ttl=300",
    "idle_keep_min_state_bytes=5",
    "tools.mount_path=/opt/other",
    "tools.seed_container.name=seeder",
    "save.paths={/work}",
    "save.ttl_seconds=90",
    "store.path_style=true",
];

fn chart() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../charts/br-sandbox-chart")
}

fn helm(overlay: Option<&Path>, sets: &[&str]) -> std::process::Output {
    let example = chart().join("ci/example-values.yaml");
    let mut arguments = vec!["template", "t"];
    let chart = chart().display().to_string();
    let files = [Some(example), overlay.map(Path::to_path_buf)];
    let files: Vec<String> = files
        .into_iter()
        .flatten()
        .flat_map(|file| ["-f".to_owned(), file.display().to_string()])
        .collect();
    arguments.push(&chart);
    arguments.extend(files.iter().map(String::as_str));
    arguments.extend(sets.iter().flat_map(|set| ["--set", *set]));
    Command::new("helm")
        .args(&arguments)
        .output()
        .unwrap_or_else(|error| panic!("helm must be installed to run this test: {error}"))
}

fn template(overlay: Option<&Path>, sets: &[&str]) -> Vec<(String, String)> {
    let output = helm(overlay, sets);
    assert!(
        output.status.success(),
        "helm template failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("helm prints UTF-8");
    let mut found: Vec<(String, String)> = serde_saphyr::from_multiple::<Value>(&text)
        .expect("helm prints YAML")
        .iter()
        .filter(|object| object["kind"] == "ConfigMap" && object["metadata"]["name"] == CONFIG_MAP)
        .map(|map| {
            let data = map["data"].as_object().expect("the ConfigMap holds data");
            assert_eq!(data.len(), 1, "the ConfigMap holds one key");
            let namespace = map["metadata"]["namespace"].as_str().expect("a namespace");
            let document = data[CONFIG_KEY].as_str().expect("the key config.json");
            (namespace.to_owned(), document.to_owned())
        })
        .collect();
    found.sort();
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

fn overlays() -> Vec<Option<PathBuf>> {
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
    std::iter::once(None)
        .chain(found.into_iter().map(Some))
        .collect()
}

fn sealed_names() -> BTreeSet<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
    let readme = std::fs::read_to_string(path).expect("the crate README");
    let mut names = BTreeSet::new();
    let rows = readme
        .lines()
        .skip_while(|line| *line != "## Configuration keys")
        .skip(1)
        .skip_while(|line| line.is_empty())
        .take_while(|line| line.starts_with('|'));
    for row in rows {
        let key_cell = row.split('|').nth(1).unwrap_or_default();
        names.extend(key_cell.split('`').skip(1).step_by(2).map(str::to_owned));
    }
    for required in ["namespace", "save_ttl_seconds", "store_credentials"] {
        assert!(names.contains(required), "the table names {required}");
    }
    names
}

fn none_document(sets: &[&str]) -> Map<String, Value> {
    let (_, text) = template(None, sets)
        .into_iter()
        .find(|(namespace, _)| namespace == "sbx-alpha-none")
        .expect("the namespace of the fixture");
    object(&text)
}

fn object(document: &str) -> Map<String, Value> {
    match serde_json::from_str(document).expect("the document is JSON") {
        Value::Object(map) => map,
        other => panic!("the document is not an object: {other}"),
    }
}

fn unknown_keys(document: &Map<String, Value>, table: &BTreeSet<String>) -> Vec<String> {
    let unknown = document.keys().filter(|key| !table.contains(key.as_str()));
    unknown.cloned().collect()
}

fn input(document: &Map<String, Value>) -> Result<SandboxConfigInput, serde_json::Error> {
    let mut full = document.clone();
    full.insert(
        "store_credentials".into(),
        json!({"access_key_id": ACCESS_KEY, "secret_access_key": SECRET_KEY}),
    );
    serde_json::from_value(Value::Object(full))
}

fn read_back(config: &SandboxConfig) -> Map<String, Value> {
    let store = config.store();
    let Value::Object(mut read) = json!({
        "namespace": config.namespace(),
        "client_id": config.client_id().to_string(),
        "label_prefix": config.label_prefix().as_str(),
        "pool_id": config.pool_id().to_string(),
        "network_mode": config.network().mode().as_str(),
        "save_ttl_seconds": config.save_ttl().as_secs(),
        "save_scope_paths": config.save_scope_paths().iter().map(|path| path.as_str()).collect::<Vec<_>>(),
        "restore_timeout_seconds": config.restore_timeout().as_secs(),
        "exec_timeout_seconds": config.exec_timeout().map(|limit| limit.as_secs()),
        "idle_ttl_seconds": config.idle_ttl().as_secs(),
        "idle_keep_min_state_bytes": config.idle_keep_min_state_bytes(),
        "agent_container": config.agent_container(),
        "seed_container": config.seed_container(),
        "tools_path": config.tools_path().as_str(),
        "seed_tools_path": config.seed_tools_path().as_str(),
        "seed_root": config.seed_root().as_str(),
        "store_endpoint": store.endpoint().as_str().trim_end_matches('/'),
        "store_bucket": store.bucket(),
        "store_region": store.region(),
        "store_path_style": store.path_style(),
    }) else {
        unreachable!("json! of an object")
    };
    if let NetworkRule::Allow(sites) = config.network() {
        read.insert(
            "allowlist".into(),
            json!(
                sites
                    .iter()
                    .map(|site| site.to_string())
                    .collect::<Vec<_>>()
            ),
        );
    }
    read
}

fn sort_allowlist(document: &mut Map<String, Value>) {
    if let Some(Value::Array(list)) = document.get_mut("allowlist") {
        list.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    }
}

fn value_differences(document: &Map<String, Value>) -> Vec<String> {
    let config = SandboxConfig::try_from(input(document).expect("the adapter reads the document"))
        .expect("the adapter accepts the document");
    let (mut rendered, mut read) = (document.clone(), read_back(&config));
    sort_allowlist(&mut rendered);
    sort_allowlist(&mut read);
    let changed = rendered
        .iter()
        .filter(|(key, value)| read.get(*key) != Some(*value));
    let absent = read.keys().filter(|key| !rendered.contains_key(*key));
    changed.map(|(key, _)| key).chain(absent).cloned().collect()
}

fn check_namespace(label: &str, text: &str, table: &BTreeSet<String>) {
    for secret in [ACCESS_KEY, SECRET_KEY, "store_credentials", "kube_context"] {
        assert!(
            !text.contains(secret),
            "{label}: the document holds {secret}"
        );
    }
    let document = object(text);
    assert_eq!(
        unknown_keys(&document, table),
        Vec::<String>::new(),
        "{label}"
    );
    if document["network_mode"] == "allow" && !document.contains_key("allowlist") {
        let loaded = SandboxConfig::try_from(input(&document).expect("the adapter reads the keys"));
        assert!(
            matches!(loaded, Err(ConfigError::AllowlistRequired)),
            "{label}: {loaded:?}"
        );
    } else {
        assert_eq!(
            value_differences(&document),
            Vec::<String>::new(),
            "{label}"
        );
    }
}

#[test]
fn every_values_file_renders_documents_that_the_adapter_reads_key_for_key() {
    let table = sealed_names();
    let mut modes = BTreeSet::new();
    for overlay in overlays() {
        let label = format!("{overlay:?}");
        for (namespace, text) in template(overlay.as_deref(), &[]) {
            check_namespace(&format!("{label} {namespace}"), &text, &table);
            modes.insert(object(&text)["network_mode"].to_string());
        }
    }
    assert_eq!(modes.len(), 3);
}

#[test]
fn a_values_change_reaches_the_key_that_the_adapter_reads() {
    let table = sealed_names();
    for (namespace, text) in template(None, &CHANGED) {
        check_namespace(&namespace, &text, &table);
        let document = object(&text);
        assert_eq!(document["restore_timeout_seconds"], 45, "{namespace}");
        assert_eq!(document["seed_container"], "seeder", "{namespace}");
        assert_eq!(
            document["tools_path"], "/opt/other/bin/bsdtar",
            "{namespace}"
        );
        assert_eq!(
            document["save_scope_paths"],
            json!(["/work"]),
            "{namespace}"
        );
    }
}

#[test]
fn a_document_with_a_required_key_missing_is_caught() {
    let mut document = none_document(&[]);
    document.remove("save_ttl_seconds");
    let error =
        input(&document).expect_err("the adapter refuses a document with no save_ttl_seconds");
    assert!(error.to_string().contains("save_ttl_seconds"), "{error}");
}

#[test]
fn a_document_with_a_misnamed_key_is_caught_by_the_table_and_by_the_values() {
    let table = sealed_names();
    let mut document = none_document(&CHANGED);
    assert!(unknown_keys(&document, &table).is_empty());
    assert!(value_differences(&document).is_empty());

    let kept = document
        .remove("restore_timeout_seconds")
        .expect("a key of the table");
    document.insert("restore_timeout".into(), kept);
    assert_eq!(unknown_keys(&document, &table), ["restore_timeout"]);
    let silently_defaulted = value_differences(&document);
    assert!(
        silently_defaulted.contains(&"restore_timeout".to_owned()),
        "{silently_defaulted:?}"
    );
    assert!(
        silently_defaulted.contains(&"restore_timeout_seconds".to_owned()),
        "{silently_defaulted:?}"
    );
}

#[test]
fn a_mode_allow_with_no_list_fails_the_render_when_egress_is_enabled() {
    let output = helm(None, &["egress.enabled=true"]);
    assert!(!output.status.success(), "the render passed");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("BR_SANDBOX_ALLOW_EMPTY{namespace=sbx-alpha-allow,mode=allow}"),
        "{error}"
    );
}

#[test]
fn a_mode_allow_with_a_list_renders_a_document_that_the_adapter_accepts() {
    let egress = chart().join("ci/egress-values.yaml");
    let documents = template(Some(&egress), &[]);
    let (_, text) = documents
        .iter()
        .find(|(namespace, _)| namespace == "sbx-alpha-allow")
        .expect("the allow namespace of the fixture");
    let document = object(text);
    assert!(document.contains_key("allowlist"));
    assert_eq!(value_differences(&document), Vec::<String>::new());
}
