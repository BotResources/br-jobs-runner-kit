use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::{
    JSONSchemaProps, JSONSchemaPropsOrArray,
};
use serde_json::{Value, json};

use super::{FloorSpec, PoolSize, SandboxPoolSpec, SizeBounds, definition};

mod definition_file;
mod drift;
mod identity;
mod schema_rules;
mod spec_reading;

fn full_spec() -> SandboxPoolSpec {
    let mut spec = SandboxPoolSpec::new(
        "ghcr.io/botresources/agent:1",
        PoolSize::new(
            SizeBounds::new("500m", "2"),
            SizeBounds::new("512Mi", "2Gi"),
            SizeBounds::new("1Gi", "4Gi"),
        ),
        1,
        8,
    );
    spec.refill_seconds = 5;
    spec.margin = 1;
    spec.floors = vec![FloorSpec::new("08:00", "18:00:30", 4)];
    spec
}

fn full_spec_json() -> Value {
    serde_json::to_value(full_spec()).unwrap()
}

fn spec_json_with(path: &[&str], key: &str) -> Value {
    let mut value = json!({
        "image": "img",
        "size": {
            "cpu": {"request": "1", "limit": "2"},
            "memory": {"request": "1Gi", "limit": "2Gi"},
            "disk": {"request": "1Gi", "limit": "2Gi"}
        },
        "minReplicas": 0,
        "maxReplicas": 3,
        "floors": [{"from": "01:00", "to": "02:00", "floor": 1}]
    });
    let mut at = &mut value;
    for step in path {
        at = match step.parse::<usize>() {
            Ok(index) => &mut at[index],
            Err(_) => &mut at[*step],
        };
    }
    at[key] = json!(1);
    value
}

fn spec_schema() -> JSONSchemaProps {
    let root = definition().spec.versions[0]
        .schema
        .clone()
        .unwrap()
        .open_api_v3_schema
        .unwrap();
    root.properties.unwrap().remove("spec").unwrap()
}

fn status_schema() -> JSONSchemaProps {
    let root = definition().spec.versions[0]
        .schema
        .clone()
        .unwrap()
        .open_api_v3_schema
        .unwrap();
    root.properties.unwrap().remove("status").unwrap()
}

fn child<'a>(schema: &'a JSONSchemaProps, step: &str) -> &'a JSONSchemaProps {
    if step == "[]" {
        match schema.items.as_ref().unwrap() {
            JSONSchemaPropsOrArray::Schema(items) => items,
            JSONSchemaPropsOrArray::Schemas(_) => panic!("an array of schemas"),
        }
    } else {
        &schema.properties.as_ref().unwrap()[step]
    }
}

fn at_path<'a>(schema: &'a JSONSchemaProps, path: &[&str]) -> &'a JSONSchemaProps {
    path.iter().fold(schema, |schema, step| child(schema, step))
}
