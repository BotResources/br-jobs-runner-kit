use br_runner_sandbox::{
    CLEAN_FALSE, CLEAN_TRUE, CONDITION_WARM_PODS_FAILING, LabelPrefix, MANAGED_TRUE, POOL_CLAIMED,
    POOL_WARM, PodKeys, REASON_IMAGE_PULL_FAILING, REASON_START_CRASHING, manifest_key, part_key,
    record_key, tombstone_key,
};

use crate::testing::saves::save_key;

fn keys(prefix: &str) -> PodKeys {
    PodKeys::new(LabelPrefix::new(prefix.to_owned()).unwrap())
}

#[test]
fn the_names_that_the_keeper_reads_are_the_names_of_the_documented_table() {
    let keys = keys("br.sandbox");
    assert_eq!(keys.pool(), "br.sandbox/pool");
    assert_eq!(keys.pool_id(), "br.sandbox/pool-id");
    assert_eq!(keys.thread(), "br.sandbox/thread");
    assert_eq!(keys.agent(), "br.sandbox/agent");
    assert_eq!(keys.clean(), "br.sandbox/clean");
    assert_eq!(keys.claimed_at(), "br.sandbox/claimed-at");
    assert_eq!(keys.idle_since(), "br.sandbox/idle-since");
    assert_eq!(keys.state_bytes(), "br.sandbox/state-bytes");
    assert_eq!(keys.managed(), "br.sandbox/managed");
}

#[test]
fn another_prefix_rewrites_every_name_and_the_default_selector_follows_it() {
    let keys = keys("acme.box");
    assert_eq!(keys.pool(), "acme.box/pool");
    assert_eq!(keys.idle_since(), "acme.box/idle-since");
    let mut document = crate::testing::document();
    document["label_prefix"] = "acme.box".into();
    let config = crate::testing::config_of(&document);
    assert_eq!(config.managed_namespace_selector, "acme.box/managed=true");
}

#[test]
fn the_values_that_the_keeper_reads_are_the_documented_values() {
    assert_eq!((POOL_WARM, POOL_CLAIMED), ("warm", "claimed"));
    assert_eq!((CLEAN_TRUE, CLEAN_FALSE), ("true", "false"));
    assert_eq!(MANAGED_TRUE, "true");
    assert_eq!(CONDITION_WARM_PODS_FAILING, "WarmPodsFailing");
    assert_eq!(REASON_IMAGE_PULL_FAILING, "ImagePullFailing");
    assert_eq!(REASON_START_CRASHING, "StartCrashing");
}

#[test]
fn the_store_keys_have_the_documented_layout() {
    let key = save_key();
    let segments = |text: String| text.split('/').map(str::to_owned).collect::<Vec<_>>();
    let manifest = segments(manifest_key(&key));
    assert_eq!((manifest[0].as_str(), manifest.len()), ("saves", 6));
    assert_eq!(manifest[5], "manifest.json");
    let record = segments(record_key(&key));
    assert_eq!((record[0].as_str(), record.len()), ("records", 5));
    assert!(record[4].ends_with(".json"));
    let tombstone = segments(tombstone_key(&key));
    assert_eq!((tombstone[0].as_str(), tombstone.len()), ("tombstones", 5));
    assert!(tombstone[4].ends_with(".json"));
    let part = segments(part_key(&key, br_runner_sandbox::AttemptId::mint()));
    assert_eq!((part[0].as_str(), part.len()), ("saves", 7));
    assert!(part[6].starts_with("part-") && part[6].ends_with(".tar.zst"));
    assert_eq!(manifest[1..5], part[1..5]);
}

fn lock_graph() -> std::collections::BTreeMap<String, Vec<String>> {
    let lock = include_str!("../../../Cargo.lock");
    let mut graph = std::collections::BTreeMap::new();
    for block in lock.split("[[package]]").skip(1) {
        let mut name = None;
        let mut dependencies = Vec::new();
        let mut in_dependencies = false;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("name = ") {
                name = Some(value.trim_matches('"').to_owned());
            } else if line.starts_with("dependencies = [") {
                in_dependencies = true;
            } else if in_dependencies && line.starts_with(']') {
                in_dependencies = false;
            } else if in_dependencies {
                let entry = line.trim().trim_end_matches(',').trim_matches('"');
                dependencies.push(entry.split(' ').next().unwrap_or_default().to_owned());
            }
        }
        if let Some(name) = name {
            graph.insert(name, dependencies);
        }
    }
    graph
}

fn reach(
    graph: &std::collections::BTreeMap<String, Vec<String>>,
    root: &str,
) -> std::collections::BTreeSet<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut queue = vec![root.to_owned()];
    while let Some(name) = queue.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        queue.extend(graph.get(&name).cloned().unwrap_or_default());
    }
    seen
}

const SYSTEM_VERIFIER: &str = "rustls-platform-verifier";

#[test]
fn the_dependency_tree_of_the_keeper_has_no_webpki_roots() {
    let seen = reach(&lock_graph(), "br-sandbox-keeper");
    assert!(seen.contains("kube"));
    assert!(seen.contains("object_store"));
    assert!(!seen.contains("webpki-roots"), "{seen:?}");
}

#[test]
fn the_clients_of_the_api_server_and_of_the_store_verify_with_the_verifier_of_the_system() {
    let graph = lock_graph();
    for client in ["kube-client", "reqwest"] {
        assert!(
            graph[client].iter().any(|name| name == SYSTEM_VERIFIER),
            "{client} has no direct dependency on {SYSTEM_VERIFIER}"
        );
    }
    assert!(reach(&graph, SYSTEM_VERIFIER).contains("rustls-native-certs"));
}
