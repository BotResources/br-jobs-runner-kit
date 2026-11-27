use serde_json::json;

use super::classes::{CLASSES, DIRECT, class_of, is_allowed};
use super::tree::Tree;

fn clean_edges() -> Vec<(&'static str, &'static str)> {
    vec![
        ("keeper", "kube"),
        ("kube", "kube-client"),
        ("kube-client", "hyper-util"),
        ("keeper", "object_store"),
        ("object_store", "reqwest"),
        ("reqwest", "h2"),
        ("keeper", "serde"),
    ]
}

fn with<'a>(extra: &[(&'a str, &'a str)]) -> Tree {
    let mut edges: Vec<(&'a str, &'a str)> = clean_edges();
    edges.extend_from_slice(extra);
    Tree::new("keeper", &edges)
}

#[test]
fn a_clean_tree_passes() {
    assert_eq!(with(&[]).network_violations(), Vec::<String>::new());
}

#[test]
fn a_telemetry_exporter_below_an_allowed_crate_fails_and_names_the_path() {
    let violations = with(&[("kube-client", "opentelemetry-otlp")]).network_violations();
    assert_eq!(violations.len(), 1);
    assert!(
        violations[0].contains("opentelemetry-otlp"),
        "{violations:?}"
    );
    assert!(violations[0].contains("keeper > kube > kube-client > opentelemetry-otlp"));
    assert!(violations[0].contains("telemetry"));
}

#[test]
fn a_client_of_every_class_fails_when_it_enters_the_tree() {
    for class in &CLASSES {
        let sample = class
            .exact
            .iter()
            .copied()
            .find(|name| !is_allowed(name))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{}x", class.prefixes[0]));
        let violations = with(&[("serde", sample.as_str())]).network_violations();
        assert_eq!(violations.len(), 1, "{} ({sample})", class.name);
        assert!(violations[0].contains(class.name), "{violations:?}");
    }
}

#[test]
fn a_prefix_matches_the_family_and_an_unrelated_name_does_not() {
    assert!(class_of("sentry-tracing").is_some());
    assert!(class_of("aws-sdk-s3").is_some());
    assert!(class_of("hyper-tls").is_some());
    assert!(class_of("hickory-resolver").is_some());
    assert!(class_of("tracing-subscriber").is_none());
    assert!(class_of("serde_json").is_none());
    assert!(class_of("rustls").is_none());
}

#[test]
fn an_allowed_crate_never_fails_and_a_second_client_next_to_it_does() {
    let violations = with(&[("keeper", "ureq"), ("keeper", "tonic")]).network_violations();
    assert_eq!(violations.len(), 2, "{violations:?}");
}

#[test]
fn a_direct_dependency_that_is_added_or_removed_fails() {
    let mut edges: Vec<(&str, &str)> = DIRECT.iter().map(|name| ("keeper", *name)).collect();
    assert_eq!(
        Tree::new("keeper", &edges).direct_violations(),
        Vec::<String>::new()
    );
    edges.push(("keeper", "ureq"));
    assert_eq!(
        Tree::new("keeper", &edges).direct_violations(),
        vec!["added ureq".to_owned()]
    );
    edges.pop();
    edges.pop();
    assert_eq!(
        Tree::new("keeper", &edges).direct_violations(),
        vec!["removed uuid".to_owned()]
    );
}

#[test]
fn only_the_normal_edges_of_the_metadata_are_followed() {
    let package = |id: &str, name: &str| json!({"id": id, "name": name, "manifest_path": format!("/x/{name}/Cargo.toml")});
    let dep = |pkg: &str, name: &str, kind: serde_json::Value| json!({"pkg": pkg, "name": name, "dep_kinds": [{"kind": kind, "target": null}]});
    let metadata = json!({
        "packages": [
            package("k", "keeper"),
            package("a", "kube"),
            package("b", "sentry"),
            package("c", "ureq"),
        ],
        "resolve": {"nodes": [
            {"id": "k", "deps": [
                dep("a", "kube", json!(null)),
                dep("b", "sentry", json!("dev")),
                dep("c", "ureq", json!("build")),
            ]},
            {"id": "a", "deps": []},
            {"id": "b", "deps": []},
            {"id": "c", "deps": []},
        ]}
    });
    let tree = Tree::from_metadata(&metadata, "/x/keeper/Cargo.toml");
    let reached: Vec<String> = tree.reach().into_keys().collect();
    assert_eq!(reached, vec!["keeper".to_owned(), "kube".to_owned()]);
}

#[test]
fn a_normal_edge_next_to_a_dev_edge_of_the_same_crate_is_followed() {
    let metadata = json!({
        "packages": [
            {"id": "k", "name": "keeper", "manifest_path": "/x/Cargo.toml"},
            {"id": "t", "name": "tokio", "manifest_path": "/y/Cargo.toml"},
        ],
        "resolve": {"nodes": [
            {"id": "k", "deps": [{"pkg": "t", "name": "tokio", "dep_kinds": [
                {"kind": null, "target": null},
                {"kind": "dev", "target": null}
            ]}]},
            {"id": "t", "deps": []},
        ]}
    });
    let tree = Tree::from_metadata(&metadata, "/x/Cargo.toml");
    assert!(tree.direct().contains("tokio"));
}
