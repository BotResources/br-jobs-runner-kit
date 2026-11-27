use std::collections::BTreeMap;

use br_runner_sandbox::LabelPrefix;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

use super::*;
use crate::pool::settings::{PoolSize, ResourceBounds};

fn keys() -> PodKeys {
    PodKeys::new(LabelPrefix::default())
}

fn template_json() -> Value {
    json!({
        "metadata": {
            "labels": {
                "br.sandbox/role": "sandbox",
                "br.sandbox/pool": "warm",
                "br.sandbox/pool-id": "sbx-acme"
            },
            "annotations": {"keep": "me"}
        },
        "spec": {
            "automountServiceAccountToken": false,
            "initContainers": [{"name": "agent", "image": "init"}],
            "containers": [
                {"name": "seed", "image": "seed:1"},
                {
                    "name": "agent",
                    "image": "placeholder",
                    "command": ["sleep", "infinity"],
                    "env": [{"name": "A", "value": "1"}],
                    "resources": {"limits": {"size": "1"}}
                }
            ],
            "volumes": [{"name": "work", "emptyDir": {"sizeLimit": "1Gi"}}]
        }
    })
}

fn config_map(text: Option<String>) -> ConfigMap {
    ConfigMap {
        metadata: ObjectMeta::default(),
        data: text.map(|text| BTreeMap::from([(POD_TEMPLATE_KEY.to_owned(), text)])),
        ..ConfigMap::default()
    }
}

fn read(value: &Value) -> Result<PoolTemplate, TemplateFault> {
    PoolTemplate::read(
        Some(&config_map(Some(value.to_string()))),
        "sbx-acme",
        &keys(),
        "agent",
    )
}

fn settings(image: &str) -> PoolSettings {
    PoolSettings {
        image: image.to_owned(),
        size: PoolSize::new(
            ResourceBounds::new("500m", "2"),
            ResourceBounds::new("1Gi", "4Gi"),
            ResourceBounds::new("1Gi", "8Gi"),
        ),
        min_replicas: 1,
        max_replicas: 4,
        refill: std::time::Duration::from_secs(3),
        margin: 2,
        floors: Vec::new(),
    }
}

fn fault_of(value: &Value) -> String {
    match read(value).unwrap_err() {
        TemplateFault::Invalid(reason) => reason,
        TemplateFault::Absent => panic!("absent"),
    }
}

#[test]
fn a_missing_config_map_is_template_absent_and_never_a_fence_matter() {
    let result = PoolTemplate::read(None, "sbx-acme", &keys(), "agent");
    assert_eq!(result.unwrap_err(), TemplateFault::Absent);
}

#[test]
fn a_good_template_is_read() {
    assert!(read(&template_json()).is_ok());
}

#[test]
fn a_config_map_with_no_key_or_a_text_that_is_not_a_json_object_is_invalid() {
    let no_key = PoolTemplate::read(Some(&config_map(None)), "sbx-acme", &keys(), "agent");
    assert!(matches!(no_key, Err(TemplateFault::Invalid(reason)) if reason.contains("pod-template.json")));
    for text in ["not json", "[1]", "\"x\"", "null"] {
        let result = PoolTemplate::read(
            Some(&config_map(Some(text.to_owned()))),
            "sbx-acme",
            &keys(),
            "agent",
        );
        assert!(matches!(result, Err(TemplateFault::Invalid(_))), "{text}");
    }
}

#[test]
fn the_pool_label_must_be_warm() {
    for value in [json!("claimed"), json!("Warm"), json!(1)] {
        let mut bad = template_json();
        bad["metadata"]["labels"]["br.sandbox/pool"] = value;
        assert!(fault_of(&bad).contains("br.sandbox/pool must be warm"));
    }
    let mut missing = template_json();
    missing["metadata"]["labels"]
        .as_object_mut()
        .unwrap()
        .remove("br.sandbox/pool");
    assert!(fault_of(&missing).contains("br.sandbox/pool must be warm"));
    let mut no_labels = template_json();
    no_labels["metadata"].as_object_mut().unwrap().remove("labels");
    assert!(fault_of(&no_labels).contains("must be warm"));
}

#[test]
fn the_pool_id_label_is_the_name_of_the_namespace() {
    let mut other = template_json();
    other["metadata"]["labels"]["br.sandbox/pool-id"] = json!("sbx-other");
    assert!(fault_of(&other).contains("br.sandbox/pool-id must be sbx-acme"));
    let mut missing = template_json();
    missing["metadata"]["labels"]
        .as_object_mut()
        .unwrap()
        .remove("br.sandbox/pool-id");
    assert!(fault_of(&missing).contains("pool-id"));
}

#[test]
fn the_labels_follow_the_label_prefix() {
    let keys = PodKeys::new(LabelPrefix::new("acme.sbx".to_owned()).unwrap());
    let value = template_json();
    let result = PoolTemplate::read(
        Some(&config_map(Some(value.to_string()))),
        "sbx-acme",
        &keys,
        "agent",
    );
    assert!(matches!(result, Err(TemplateFault::Invalid(reason)) if reason.contains("acme.sbx/pool")));
}

#[test]
fn a_container_with_the_name_of_the_agent_container_must_exist_and_an_init_container_does_not_count() {
    let mut bad = template_json();
    bad["spec"]["containers"] = json!([{"name": "seed", "image": "x"}]);
    assert!(fault_of(&bad).contains("no container is named agent"));
    let mut none = template_json();
    none["spec"].as_object_mut().unwrap().remove("containers");
    assert!(fault_of(&none).contains("no container is named agent"));
    let other_name = PoolTemplate::read(
        Some(&config_map(Some(template_json().to_string()))),
        "sbx-acme",
        &keys(),
        "sandbox",
    );
    assert!(matches!(other_name, Err(TemplateFault::Invalid(_))));
}

#[test]
fn the_keeper_changes_the_image_and_the_resources_of_the_agent_container_and_the_annotation_only() {
    let template = read(&template_json()).unwrap();
    let rendered = template.render(&settings("registry.example/agent:2"), &keys());
    let mut expected = template_json();
    expected["spec"]["containers"][1]["image"] = json!("registry.example/agent:2");
    expected["spec"]["containers"][1]["resources"] = json!({
        "requests": {"cpu": "500m", "memory": "1Gi", "ephemeral-storage": "1Gi"},
        "limits": {"cpu": "2", "memory": "4Gi", "ephemeral-storage": "8Gi"}
    });
    expected["metadata"]["annotations"]["br.sandbox/template-hash"] = json!(rendered.hash);
    assert_eq!(rendered.template, expected);
}

#[test]
fn the_annotation_is_added_when_the_template_has_no_annotation() {
    let mut value = template_json();
    value["metadata"].as_object_mut().unwrap().remove("annotations");
    let rendered = read(&value).unwrap().render(&settings("i:1"), &keys());
    assert_eq!(
        rendered.template["metadata"]["annotations"]["br.sandbox/template-hash"],
        json!(rendered.hash)
    );
}

#[test]
fn the_hash_is_64_hex_characters_and_the_same_for_the_same_input() {
    let template = read(&template_json()).unwrap();
    let first = template.render(&settings("i:1"), &keys());
    let second = template.render(&settings("i:1"), &keys());
    assert_eq!(first.hash, second.hash);
    assert_eq!(first.hash.len(), 64);
    assert!(first.hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn the_hash_changes_with_the_image_the_size_and_any_field_of_the_config_map() {
    let base = read(&template_json()).unwrap().render(&settings("i:1"), &keys());
    let image = read(&template_json()).unwrap().render(&settings("i:2"), &keys());
    assert_ne!(base.hash, image.hash);
    let mut bigger = settings("i:1");
    bigger.size.cpu = ResourceBounds::new("500m", "3");
    let size = read(&template_json()).unwrap().render(&bigger, &keys());
    assert_ne!(base.hash, size.hash);
    let mut edited = template_json();
    edited["spec"]["containers"][1]["env"][0]["value"] = json!("2");
    let config = read(&edited).unwrap().render(&settings("i:1"), &keys());
    assert_ne!(base.hash, config.hash);
}

#[test]
fn the_hash_does_not_change_with_the_order_of_keys_or_with_a_hash_already_in_the_config_map() {
    let base = read(&template_json()).unwrap().render(&settings("i:1"), &keys());
    let text = template_json().to_string();
    let reordered: Value = {
        let mut value: Value = serde_json::from_str(&text).unwrap();
        let spec = value["spec"].take();
        let metadata = value["metadata"].take();
        json!({"spec": spec, "metadata": metadata})
    };
    let same = read(&reordered).unwrap().render(&settings("i:1"), &keys());
    assert_eq!(base.hash, same.hash);
    let mut stamped = template_json();
    stamped["metadata"]["annotations"]["br.sandbox/template-hash"] = json!("old");
    let ignored = read(&stamped).unwrap().render(&settings("i:1"), &keys());
    assert_eq!(base.hash, ignored.hash);
    assert_eq!(base.template, ignored.template);
}

#[test]
fn the_resources_of_the_config_map_are_replaced_whole_by_the_size_of_the_resource() {
    let rendered = read(&template_json()).unwrap().render(&settings("i:1"), &keys());
    assert!(rendered.template["spec"]["containers"][1]["resources"]["limits"]["size"].is_null());
    assert_eq!(
        rendered.template["spec"]["containers"][0],
        json!({"name": "seed", "image": "seed:1"})
    );
    assert_eq!(
        rendered.template["spec"]["initContainers"],
        template_json()["spec"]["initContainers"]
    );
}
