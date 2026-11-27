use serde_json::{Value, json};

use super::*;

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

#[test]
fn a_template_with_another_hash_or_none_is_not_in_place() {
    assert!(!template_in_place(
        &replica_set(Some("h0"), vec![]),
        KEY,
        "h1"
    ));
    assert!(!template_in_place(&replica_set(None, vec![]), KEY, "h1"));
}

#[test]
fn a_template_with_the_hash_and_no_entry_of_the_old_manager_is_in_place() {
    assert!(in_place(&replica_set(Some("h1"), vec![])));
    let created = entry(FIELD_MANAGER, "Update", None, template_fields());
    assert!(in_place(&replica_set(Some("h1"), vec![created])));
}

#[test]
fn a_template_that_the_old_manager_applied_and_the_new_manager_did_not_is_not_in_place() {
    let legacy = entry(FIELD_MANAGER, "Apply", None, template_fields());
    assert!(!in_place(&replica_set(Some("h1"), vec![legacy])));
}

#[test]
fn a_template_that_the_new_manager_applied_is_in_place_with_or_without_the_old_entry() {
    let legacy = entry(FIELD_MANAGER, "Apply", None, template_fields());
    let current = entry(TEMPLATE_FIELD_MANAGER, "Apply", None, template_fields());
    assert!(in_place(&replica_set(
        Some("h1"),
        vec![legacy, current.clone()]
    )));
    assert!(in_place(&replica_set(Some("h1"), vec![current])));
}

#[test]
fn an_apply_entry_of_the_old_manager_that_holds_no_template_field_is_not_a_migration() {
    let replicas = entry(FIELD_MANAGER, "Apply", None, replicas_fields());
    assert!(in_place(&replica_set(Some("h1"), vec![replicas])));
}

#[test]
fn the_status_entry_of_the_old_manager_is_not_a_migration() {
    let status = entry(FIELD_MANAGER, "Apply", Some("status"), template_fields());
    assert!(in_place(&replica_set(Some("h1"), vec![status])));
}
