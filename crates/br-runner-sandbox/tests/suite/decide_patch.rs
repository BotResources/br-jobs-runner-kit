use br_runner_sandbox::{
    LabelPrefix, PodKeys, PoolId, Timestamp, cold_pod_marks, idle_patch, live_claim_patch,
    saved_patch, seed_hash_patch, warm_claim_patch,
};
use serde_json::{Value, json};

use crate::common::{agent, thread, turn};

fn keys() -> PodKeys {
    PodKeys::default()
}

fn labels(patch: &Value) -> Vec<(String, Value)> {
    members(&patch["metadata"]["labels"])
}

fn annotations(patch: &Value) -> Vec<(String, Value)> {
    members(&patch["metadata"]["annotations"])
}

fn members(value: &Value) -> Vec<(String, Value)> {
    value
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn get(list: &[(String, Value)], key: &str) -> Option<Value> {
    list.iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
}

#[test]
fn the_claim_patch_of_a_warm_pod_carries_the_version_that_was_read() {
    let patch = warm_claim_patch(&keys(), "4711", thread(), agent(), Timestamp::now());
    assert_eq!(patch["metadata"]["resourceVersion"], "4711");
}

#[test]
fn the_claim_patch_of_a_warm_pod_sets_the_pool_the_thread_the_agent_clean_and_the_time() {
    let (thread, agent) = (thread(), agent());
    let now = Timestamp::now();
    let patch = warm_claim_patch(&keys(), "1", thread, agent, now);
    let labels = labels(&patch);
    let annotations = annotations(&patch);
    assert_eq!(get(&labels, "br.sandbox/pool"), Some(json!("claimed")));
    assert_eq!(
        get(&labels, "br.sandbox/thread"),
        Some(json!(thread.to_string()))
    );
    assert_eq!(
        get(&labels, "br.sandbox/agent"),
        Some(json!(agent.to_string()))
    );
    assert_eq!(labels.len(), 3);
    assert_eq!(get(&annotations, "br.sandbox/clean"), Some(json!("false")));
    assert_eq!(
        get(&annotations, "br.sandbox/claimed-at"),
        Some(json!(now.to_string()))
    );
    assert_eq!(annotations.len(), 2);
}

#[test]
fn a_claim_changes_no_other_key_than_the_labels_and_annotations_of_the_contract() {
    let patches = [
        warm_claim_patch(&keys(), "1", thread(), agent(), Timestamp::now()),
        live_claim_patch(&keys(), "1", Timestamp::now()),
    ];
    for patch in patches {
        let top: Vec<_> = members(&patch).into_iter().map(|(key, _)| key).collect();
        assert_eq!(top, vec!["metadata"]);
        let mut meta: Vec<_> = members(&patch["metadata"])
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        meta.sort_unstable();
        assert!(
            meta.iter()
                .all(|key| matches!(key.as_str(), "resourceVersion" | "labels" | "annotations"))
        );
        for (key, _) in labels(&patch).into_iter().chain(annotations(&patch)) {
            assert!(key.starts_with("br.sandbox/"), "{key}");
        }
    }
}

#[test]
fn a_claim_never_changes_the_role_nor_the_pool_id_and_never_puts_a_pod_back_in_the_pool() {
    let patches = [
        warm_claim_patch(&keys(), "1", thread(), agent(), Timestamp::now()),
        live_claim_patch(&keys(), "1", Timestamp::now()),
        saved_patch(&keys(), turn(), 1, Timestamp::now()),
        idle_patch(&keys(), Timestamp::now()),
        seed_hash_patch(&keys(), "h"),
    ];
    for patch in patches {
        let text = patch.to_string();
        assert!(!text.contains("br.sandbox/role") && !text.contains("br.sandbox/pool-id"));
        assert!(!text.contains("\"warm\""));
        assert!(!text.contains("creds"));
    }
}

#[test]
fn the_claim_patch_of_a_live_pod_sets_clean_and_the_time_and_removes_idle_since_and_state_bytes() {
    let now = Timestamp::now();
    let patch = live_claim_patch(&keys(), "9", now);
    assert_eq!(patch["metadata"]["resourceVersion"], "9");
    assert!(labels(&patch).is_empty());
    let annotations = annotations(&patch);
    assert_eq!(get(&annotations, "br.sandbox/clean"), Some(json!("false")));
    assert_eq!(
        get(&annotations, "br.sandbox/claimed-at"),
        Some(json!(now.to_string()))
    );
    assert_eq!(
        get(&annotations, "br.sandbox/idle-since"),
        Some(Value::Null)
    );
    assert_eq!(
        get(&annotations, "br.sandbox/state-bytes"),
        Some(Value::Null)
    );
}

#[test]
fn the_claim_patch_of_a_live_pod_leaves_the_state_turn_the_save_time_and_the_seed_hash() {
    let text = live_claim_patch(&keys(), "9", Timestamp::now()).to_string();
    for key in ["state-turn", "saved-at", "seed-hash"] {
        assert!(!text.contains(key), "{key}");
    }
}

#[test]
fn the_save_patch_sets_clean_the_turn_the_bytes_and_the_time_and_not_idle_since() {
    let (turn, made_at) = (turn(), Timestamp::now());
    let patch = saved_patch(&keys(), turn, 12345, made_at);
    let annotations = annotations(&patch);
    assert_eq!(get(&annotations, "br.sandbox/clean"), Some(json!("true")));
    assert_eq!(
        get(&annotations, "br.sandbox/state-turn"),
        Some(json!(turn.to_string()))
    );
    assert_eq!(
        get(&annotations, "br.sandbox/state-bytes"),
        Some(json!("12345"))
    );
    assert_eq!(
        get(&annotations, "br.sandbox/saved-at"),
        Some(json!(made_at.to_string()))
    );
    assert_eq!(annotations.len(), 4);
    assert!(patch["metadata"].get("resourceVersion").is_none());
}

#[test]
fn the_release_patch_sets_idle_since_only() {
    let now = Timestamp::now();
    let patch = idle_patch(&keys(), now);
    assert_eq!(
        annotations(&patch),
        vec![("br.sandbox/idle-since".to_owned(), json!(now.to_string()))]
    );
}

#[test]
fn the_seed_patch_sets_the_seed_hash_text_only() {
    let patch = seed_hash_patch(&keys(), "abc123");
    assert_eq!(
        annotations(&patch),
        vec![("br.sandbox/seed-hash".to_owned(), json!("abc123"))]
    );
}

#[test]
fn a_cold_pod_has_the_claim_keys_from_its_creation_so_that_the_keeper_counts_it_as_a_claim() {
    let (thread, agent, now) = (thread(), agent(), Timestamp::now());
    let marks = cold_pod_marks(
        &keys(),
        &PoolId::new("main").expect("pool"),
        thread,
        agent,
        now,
    );
    assert_eq!(
        marks.labels.get("br.sandbox/pool-id").map(String::as_str),
        Some("main")
    );
    assert_eq!(
        marks.labels.get("br.sandbox/pool").map(String::as_str),
        Some("claimed")
    );
    assert_eq!(
        marks.labels.get("br.sandbox/thread"),
        Some(&thread.to_string())
    );
    assert_eq!(
        marks.labels.get("br.sandbox/agent"),
        Some(&agent.to_string())
    );
    assert_eq!(marks.labels.len(), 4);
    assert_eq!(
        marks
            .annotations
            .get("br.sandbox/clean")
            .map(String::as_str),
        Some("false")
    );
    assert_eq!(
        marks.annotations.get("br.sandbox/claimed-at"),
        Some(&now.to_string())
    );
    assert_eq!(marks.annotations.len(), 2);
    assert!(!marks.labels.contains_key("br.sandbox/role"));
}

#[test]
fn the_patches_follow_the_label_prefix_of_the_configuration() {
    let keys = PodKeys::new(LabelPrefix::new("acme.io").expect("prefix"));
    let patch = warm_claim_patch(&keys, "1", thread(), agent(), Timestamp::now());
    for (key, _) in labels(&patch).into_iter().chain(annotations(&patch)) {
        assert!(key.starts_with("acme.io/"), "{key}");
    }
    assert!(!patch.to_string().contains("br.sandbox"));
}
