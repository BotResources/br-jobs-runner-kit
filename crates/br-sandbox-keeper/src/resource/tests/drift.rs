use std::collections::BTreeSet;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::{Condition, Time};
use serde_json::Value;

use super::super::SandboxPoolStatus;
use super::{at_path, full_spec_json, spec_schema, status_schema};

fn keys_of(value: &Value) -> BTreeSet<String> {
    value.as_object().unwrap().keys().cloned().collect()
}

fn schema_keys(
    schema: &k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::JSONSchemaProps,
) -> BTreeSet<String> {
    schema
        .properties
        .as_ref()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

#[test]
fn each_object_level_of_the_type_has_the_keys_of_its_schema() {
    let spec = full_spec_json();
    let schema = spec_schema();
    let levels: [(&[&str], &Value); 6] = [
        (&[], &spec),
        (&["size"], &spec["size"]),
        (&["size", "cpu"], &spec["size"]["cpu"]),
        (&["size", "memory"], &spec["size"]["memory"]),
        (&["size", "disk"], &spec["size"]["disk"]),
        (&["floors", "[]"], &spec["floors"][0]),
    ];
    for (path, value) in levels {
        assert_eq!(
            keys_of(value),
            schema_keys(at_path(&schema, path)),
            "{path:?}"
        );
    }
}

#[test]
fn the_size_bounds_of_the_type_are_the_request_and_the_limit_of_each_resource() {
    let spec = full_spec_json();
    for resource in ["cpu", "memory", "disk"] {
        let bounds = &spec["size"][resource];
        assert_eq!(
            keys_of(bounds),
            ["limit", "request"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
    }
}

#[test]
fn the_status_of_the_type_has_the_keys_of_its_schema() {
    let condition = Condition {
        type_: "Accepted".to_owned(),
        status: "True".to_owned(),
        reason: "Valid".to_owned(),
        message: "ok".to_owned(),
        last_transition_time: Time(jiff::Timestamp::UNIX_EPOCH),
        observed_generation: Some(2),
    };
    let status = SandboxPoolStatus {
        observed_generation: Some(2),
        conditions: vec![condition],
        desired_replicas: Some(3),
        replicas: Some(3),
        ready_replicas: Some(2),
        updated_replicas: Some(3),
        observed_at: Some(Time(jiff::Timestamp::UNIX_EPOCH)),
    };
    let value = serde_json::to_value(&status).unwrap();
    let schema = status_schema();
    assert_eq!(keys_of(&value), schema_keys(&schema));
    assert_eq!(
        keys_of(&value["conditions"][0]),
        schema_keys(at_path(&schema, &["conditions", "[]"]))
    );
}

#[test]
fn a_status_with_no_observation_writes_no_key() {
    let value = serde_json::to_value(SandboxPoolStatus::default()).unwrap();
    assert_eq!(value, serde_json::json!({}));
}
