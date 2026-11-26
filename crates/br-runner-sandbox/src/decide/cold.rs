use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::cluster::ClusterError;

use super::patch::ColdPodMarks;

fn strings(template: &Value, section: &str) -> BTreeMap<String, String> {
    template
        .pointer(&format!("/metadata/{section}"))
        .and_then(Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

fn object(entries: BTreeMap<String, String>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key, Value::String(value)))
            .collect::<Map<String, Value>>(),
    )
}

pub fn cold_pod_manifest(
    template: &Value,
    generate_name: &str,
    marks: &ColdPodMarks,
) -> Result<Value, ClusterError> {
    let spec = template
        .get("spec")
        .filter(|spec| spec.is_object())
        .ok_or_else(|| ClusterError::Config("the pool template holds no pod spec".to_owned()))?;
    let mut labels = strings(template, "labels");
    labels.extend(marks.labels.clone());
    let mut annotations = strings(template, "annotations");
    annotations.extend(marks.annotations.clone());
    Ok(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "generateName": generate_name,
            "labels": object(labels),
            "annotations": object(annotations),
        },
        "spec": spec,
    }))
}
