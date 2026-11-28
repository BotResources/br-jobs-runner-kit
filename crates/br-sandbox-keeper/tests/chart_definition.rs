use std::path::PathBuf;

use serde_json::{Number, Value};

mod common;

use common::{chart, rendered};

fn chart_file() -> PathBuf {
    chart().join("files/sandboxpool-crd.yaml")
}

fn keeper_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("definition/sandboxpools.yaml")
}

fn read(path: &PathBuf) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn with_numbers_as_floats(value: Value) -> Value {
    match value {
        Value::Number(number) => number
            .as_f64()
            .and_then(Number::from_f64)
            .map_or(Value::Number(number), Value::Number),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(with_numbers_as_floats).collect())
        }
        Value::Object(entries) => Value::Object(
            entries
                .into_iter()
                .map(|(key, item)| (key, with_numbers_as_floats(item)))
                .collect(),
        ),
        other => other,
    }
}

fn comparable(mut definition: Value) -> Value {
    let metadata = definition["metadata"]
        .as_object_mut()
        .expect("the definition has metadata");
    metadata.remove("annotations");
    metadata.remove("labels");
    with_numbers_as_floats(definition)
}

#[test]
fn the_definition_file_of_the_chart_equals_the_committed_file_of_the_keeper_byte_for_byte() {
    assert_eq!(read(&chart_file()), read(&keeper_file()));
}

#[test]
fn the_definition_file_of_the_chart_equals_what_the_keeper_generates() {
    let generated = br_sandbox_keeper::resource::definition_yaml().expect("the keeper generates");
    assert_eq!(read(&chart_file()), generated.into_bytes());
}

#[test]
fn the_definition_that_the_chart_renders_equals_the_file_but_for_the_annotation_and_the_labels() {
    let objects = rendered(&chart().join("ci/crds-values.yaml"), &[]);
    let definitions: Vec<Value> = objects
        .into_iter()
        .filter(|object| object["kind"] == "CustomResourceDefinition")
        .collect();
    assert_eq!(definitions.len(), 1);
    let file: Value =
        serde_saphyr::from_str(&String::from_utf8(read(&chart_file())).expect("the file is UTF-8"))
            .expect("the file is YAML");
    assert_eq!(comparable(definitions[0].clone()), comparable(file));
    assert_eq!(
        definitions[0]["metadata"]["annotations"]["helm.sh/resource-policy"],
        "keep"
    );
}
