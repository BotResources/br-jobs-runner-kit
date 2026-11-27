use serde_json::{Value, json};

use super::*;
use crate::cluster::FIELD_MANAGER;

const KEY: &str = "br.sandbox/template-hash";

fn entry(manager: &str, operation: &str, subresource: Option<&str>, fields: Value) -> Value {
    json!({
        "manager": manager,
        "operation": operation,
        "subresource": subresource,
        "fieldsType": "FieldsV1",
        "fieldsV1": fields
    })
}

fn template_fields() -> Value {
    json!({"f:spec": {"f:template": {"f:metadata": {"f:annotations": {"f:br.sandbox/template-hash": {}}}}}})
}

fn replicas_fields() -> Value {
    json!({"f:spec": {"f:replicas": {}}})
}

fn replica_set(hash: Option<&str>, entries: Vec<Value>) -> ReplicaSet {
    with_managed_fields(hash, json!(entries))
}

fn without_managed_fields(hash: Option<&str>) -> ReplicaSet {
    with_managed_fields(hash, Value::Null)
}

fn with_managed_fields(hash: Option<&str>, entries: Value) -> ReplicaSet {
    let annotations = hash.map(|hash| json!({KEY: hash})).unwrap_or(json!({}));
    serde_json::from_value(json!({
        "metadata": {"name": "warm-sbx", "managedFields": entries},
        "spec": {
            "selector": {},
            "template": {"metadata": {"annotations": annotations}}
        }
    }))
    .unwrap()
}

fn in_place(set: &ReplicaSet) -> bool {
    template_in_place(set, KEY, "h1")
}

fn current_entry() -> Value {
    entry(TEMPLATE_FIELD_MANAGER, "Apply", None, template_fields())
}

#[test]
fn a_template_with_another_hash_or_none_is_not_in_place_whatever_the_entries() {
    for entries in [vec![], vec![current_entry()]] {
        assert!(!template_in_place(
            &replica_set(Some("h0"), entries.clone()),
            KEY,
            "h1"
        ));
        assert!(!template_in_place(&replica_set(None, entries), KEY, "h1"));
    }
    assert!(!template_in_place(&without_managed_fields(None), KEY, "h1"));
}

#[test]
fn a_template_with_the_hash_and_the_entry_of_the_template_manager_is_in_place() {
    assert!(in_place(&replica_set(Some("h1"), vec![current_entry()])));
}

#[test]
fn the_entry_of_the_template_manager_decides_with_or_without_the_entries_of_the_old_manager() {
    let legacy = entry(FIELD_MANAGER, "Apply", None, template_fields());
    let created = entry(FIELD_MANAGER, "Update", None, template_fields());
    assert!(in_place(&replica_set(
        Some("h1"),
        vec![created, legacy, current_entry()]
    )));
}

#[test]
fn a_template_that_the_old_manager_applied_and_the_template_manager_did_not_is_not_in_place() {
    let legacy = entry(FIELD_MANAGER, "Apply", None, template_fields());
    assert!(!in_place(&replica_set(Some("h1"), vec![legacy])));
}

#[test]
fn a_replica_set_that_the_keeper_made_with_the_hash_is_not_in_place_until_the_template_manager_wrote()
 {
    let created = entry(FIELD_MANAGER, "Update", None, template_fields());
    assert!(!in_place(&replica_set(Some("h1"), vec![created])));
}

#[test]
fn a_replica_set_that_the_previous_version_damaged_is_not_in_place_whatever_the_old_entries_hold() {
    let replicas = entry(FIELD_MANAGER, "Apply", None, replicas_fields());
    let created = entry(FIELD_MANAGER, "Update", None, replicas_fields());
    assert!(!in_place(&replica_set(Some("h1"), vec![replicas.clone()])));
    assert!(!in_place(&replica_set(Some("h1"), vec![created, replicas])));
}

#[test]
fn an_entry_of_the_template_manager_that_is_no_template_apply_does_not_hold_the_template() {
    let replicas = entry(TEMPLATE_FIELD_MANAGER, "Apply", None, replicas_fields());
    let update = entry(TEMPLATE_FIELD_MANAGER, "Update", None, template_fields());
    let status = entry(
        TEMPLATE_FIELD_MANAGER,
        "Apply",
        Some("status"),
        template_fields(),
    );
    for other in [replicas, update, status] {
        assert!(!in_place(&replica_set(Some("h1"), vec![other])));
    }
}

#[test]
fn the_status_entry_of_the_old_manager_does_not_hold_the_template() {
    let status = entry(FIELD_MANAGER, "Apply", Some("status"), template_fields());
    assert!(!in_place(&replica_set(Some("h1"), vec![status])));
}

#[test]
fn a_replica_set_read_without_managed_fields_is_judged_by_the_hash_alone() {
    assert!(in_place(&without_managed_fields(Some("h1"))));
    assert!(in_place(&replica_set(Some("h1"), vec![])));
    assert!(!template_in_place(
        &without_managed_fields(Some("h0")),
        KEY,
        "h1"
    ));
}
