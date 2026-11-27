use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde_json::{Value, json};

use super::super::{
    DEFAULT_MARGIN, DEFAULT_REFILL_SECONDS, SandboxPool, SandboxPoolSpec, SizeBounds,
};
use super::{full_spec, full_spec_json, spec_json_with};

fn read(value: Value) -> Result<SandboxPoolSpec, String> {
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[test]
fn a_full_spec_goes_through_json_and_back() {
    assert_eq!(read(full_spec_json()).unwrap(), full_spec());
}

#[test]
fn an_absent_refill_margin_and_floors_take_the_defaults_of_the_table() {
    let mut value = full_spec_json();
    let object = value.as_object_mut().unwrap();
    object.remove("refillSeconds");
    object.remove("margin");
    object.remove("floors");
    let spec = read(value).unwrap();
    assert_eq!(spec.refill_seconds, 3);
    assert_eq!(spec.margin, 2);
    assert_eq!(DEFAULT_REFILL_SECONDS, 3);
    assert_eq!(DEFAULT_MARGIN, 2);
    assert!(spec.floors.is_empty());
}

#[test]
fn zero_is_allowed_for_the_replicas_the_refill_the_margin_and_a_floor() {
    let mut value = full_spec_json();
    value["minReplicas"] = json!(0);
    value["maxReplicas"] = json!(0);
    value["refillSeconds"] = json!(0);
    value["margin"] = json!(0);
    value["floors"][0]["floor"] = json!(0);
    let spec = read(value).unwrap();
    assert_eq!(spec.min_replicas, 0);
    assert_eq!(spec.max_replicas, 0);
    assert_eq!(spec.refill_seconds, 0);
    assert_eq!(spec.margin, 0);
    assert_eq!(spec.floors[0].floor, 0);
}

#[test]
fn an_unknown_key_is_refused_at_the_spec_level() {
    let error = read(spec_json_with(&[], "typoField")).unwrap_err();
    assert!(error.contains("unknown field `typoField`"), "{error}");
}

#[test]
fn an_unknown_key_is_refused_in_the_size_object() {
    let error = read(spec_json_with(&["size"], "sizeTypo")).unwrap_err();
    assert!(error.contains("unknown field `sizeTypo`"), "{error}");
}

#[test]
fn an_unknown_key_is_refused_in_each_size_sub_object() {
    for resource in ["cpu", "memory", "disk"] {
        let error = read(spec_json_with(&["size", resource], "cpuu")).unwrap_err();
        assert!(
            error.contains("unknown field `cpuu`"),
            "{resource}: {error}"
        );
    }
}

#[test]
fn an_unknown_key_is_refused_in_an_item_of_the_floors() {
    let error = read(spec_json_with(&["floors", "0"], "typo")).unwrap_err();
    assert!(error.contains("unknown field `typo`"), "{error}");
}

#[test]
fn a_missing_required_key_is_refused() {
    for key in ["image", "size", "minReplicas", "maxReplicas"] {
        let mut value = full_spec_json();
        value.as_object_mut().unwrap().remove(key);
        let error = read(value).unwrap_err();
        assert!(
            error.contains(&format!("missing field `{key}`")),
            "{key}: {error}"
        );
    }
}

#[test]
fn a_value_of_the_wrong_type_is_refused_and_never_repaired() {
    for (key, bad) in [
        ("image", json!(5)),
        ("minReplicas", json!(-1)),
        ("maxReplicas", json!(1.5)),
        ("refillSeconds", json!("3")),
        ("margin", Value::Null),
    ] {
        let mut value = full_spec_json();
        value[key] = bad;
        assert!(read(value).is_err(), "{key}");
    }
    let mut value = full_spec_json();
    value["floors"][0]["floor"] = json!("x");
    assert!(read(value).is_err());
    let mut value = full_spec_json();
    value["size"]["cpu"]["request"] = json!(true);
    assert!(read(value).is_err());
}

#[test]
fn a_bad_spec_does_not_fail_the_resource_that_holds_it() {
    let list = json!({"items": [
        {"metadata": {"name": "a"}, "spec": spec_json_with(&[], "typoField")},
        {"metadata": {"name": "b"}, "spec": full_spec_json()},
        {"metadata": {"name": "c"}}
    ]});
    let pools: Vec<SandboxPool> = serde_json::from_value(list["items"].clone()).unwrap();
    assert!(pools[0].read_spec().is_err());
    assert_eq!(pools[1].read_spec().unwrap(), full_spec());
    assert!(pools[2].read_spec().is_err());
}

#[test]
fn the_resource_keeps_the_status_and_the_name() {
    let value = json!({
        "metadata": {"name": "p", "namespace": "sbx", "generation": 2},
        "spec": full_spec_json(),
        "status": {"observedGeneration": 2, "desiredReplicas": 3}
    });
    let pool: SandboxPool = serde_json::from_value(value).unwrap();
    assert_eq!(pool.metadata.name.as_deref(), Some("p"));
    let status = pool.status.as_ref().unwrap();
    assert_eq!(status.observed_generation, Some(2));
    assert_eq!(status.desired_replicas, Some(3));
    assert_eq!(status.ready_replicas, None);
}

#[test]
fn a_resource_built_from_a_spec_reads_back_the_same_spec() {
    let pool = SandboxPool::new(ObjectMeta::default(), &full_spec()).unwrap();
    assert_eq!(pool.read_spec().unwrap(), full_spec());
    assert_eq!(SizeBounds::new("1", "2").request.0, "1");
}
