use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use serde_json::Value;

use super::super::{definition, definition_yaml};

const CHECKED_IN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/definition/sandboxpools.yaml");

fn checked_in_text() -> String {
    std::fs::read_to_string(CHECKED_IN).expect("the checked-in definition file")
}

#[test]
fn the_checked_in_definition_file_is_the_one_that_the_type_generates() {
    let checked_in: Value = serde_saphyr::from_str(&checked_in_text()).unwrap();
    let generated = serde_json::to_value(definition()).unwrap();
    assert_eq!(
        checked_in,
        generated,
        "regenerate definition/sandboxpools.yaml from definition_yaml():\n{}",
        definition_yaml().unwrap()
    );
}

#[test]
fn the_checked_in_definition_file_is_written_as_the_generated_text() {
    assert_eq!(checked_in_text(), definition_yaml().unwrap());
}

#[test]
fn the_generated_text_reads_back_as_the_same_definition() {
    let back: CustomResourceDefinition =
        serde_saphyr::from_str(&definition_yaml().unwrap()).unwrap();
    assert_eq!(back, definition());
}

#[test]
fn the_checked_in_definition_file_holds_the_mark_and_the_bounds() {
    let text = checked_in_text();
    assert_eq!(
        text.matches("x-kubernetes-preserve-unknown-fields: true")
            .count(),
        7
    );
    assert!(text.contains("maxItems: 16"));
    assert_eq!(text.matches("maxLength: 64").count(), 2);
}
