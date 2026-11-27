use std::time::Duration;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use serde_json::json;

use super::*;
use crate::resource::{PoolSize as SpecSize, SizeBounds};

fn spec() -> SandboxPoolSpec {
    SandboxPoolSpec::new(
        "registry.example/agent:1",
        SpecSize::new(
            SizeBounds::new("500m", "2"),
            SizeBounds::new("1Gi", "4Gi"),
            SizeBounds::new("1Gi", "8Gi"),
        ),
        1,
        20,
    )
}

fn refused(spec: &SandboxPoolSpec) -> InvalidSpec {
    validate_spec(spec).unwrap_err()
}

fn window(from: &str, to: &str) -> FloorSpec {
    FloorSpec::new(from, to, 5)
}

#[test]
fn a_good_spec_gives_the_settings_with_the_defaults_of_the_table() {
    let settings = validate_spec(&spec()).unwrap();
    assert_eq!(settings.image, "registry.example/agent:1");
    assert_eq!(settings.min_replicas, 1);
    assert_eq!(settings.max_replicas, 20);
    assert_eq!(settings.refill, Duration::from_secs(3));
    assert_eq!(settings.margin, 2);
    assert!(settings.floors.is_empty());
    assert_eq!(settings.size.cpu.request.0, "500m");
    assert_eq!(settings.size.disk.limit.0, "8Gi");
}

#[test]
fn the_image_must_not_be_blank() {
    for image in [
        "",
        "   ",
        "\t",
        "\u{000B}",
        "\u{00A0}",
        " \u{000B}\u{00A0}\t",
    ] {
        let mut bad = spec();
        bad.image = image.to_owned();
        assert_eq!(refused(&bad).field, "image");
    }
}

#[test]
fn a_request_above_its_limit_is_refused_for_each_of_the_three_sizes() {
    for name in ["cpu", "memory", "disk"] {
        let mut bad = spec();
        let bounds = match name {
            "cpu" => &mut bad.size.cpu,
            "memory" => &mut bad.size.memory,
            _ => &mut bad.size.disk,
        };
        *bounds = SizeBounds::new("2", "1");
        let error = refused(&bad);
        assert_eq!(error.field, format!("size.{name}.request"));
        assert!(error.reason.contains("above the limit"), "{error}");
    }
}

#[test]
fn a_request_equal_to_its_limit_is_good_and_units_are_compared_by_value() {
    let mut good = spec();
    good.size.cpu = SizeBounds::new("1", "1000m");
    good.size.memory = SizeBounds::new("1024Mi", "1Gi");
    assert!(validate_spec(&good).is_ok());
    let mut bad = spec();
    bad.size.memory = SizeBounds::new("1Gi", "1G");
    assert_eq!(refused(&bad).field, "size.memory.request");
}

#[test]
fn a_quantity_of_zero_or_below_or_not_a_quantity_is_refused_in_the_request_and_the_limit() {
    for text in ["0", "0m", "0Gi", "-1", "abc", "", "1 Gi"] {
        let mut request = spec();
        request.size.cpu = SizeBounds::new(text, "2");
        assert_eq!(refused(&request).field, "size.cpu.request", "{text:?}");
        let mut limit = spec();
        limit.size.disk = SizeBounds::new("1", text);
        assert_eq!(refused(&limit).field, "size.disk.limit", "{text:?}");
    }
}

#[test]
fn a_quantity_that_is_too_large_is_refused() {
    let mut bad = spec();
    bad.size.cpu = SizeBounds::new("1", "1e100");
    let error = refused(&bad);
    assert_eq!(error.field, "size.cpu.limit");
    assert!(error.reason.contains("too large"), "{error}");
}

#[test]
fn min_replicas_above_max_replicas_is_refused_and_equal_is_good() {
    let mut bad = spec();
    bad.min_replicas = 21;
    assert_eq!(refused(&bad).field, "minReplicas");
    let mut equal = spec();
    equal.min_replicas = 20;
    assert!(validate_spec(&equal).is_ok());
}

#[test]
fn zero_is_allowed_for_the_replicas_the_refill_the_margin_and_a_floor() {
    let mut zero = spec();
    zero.min_replicas = 0;
    zero.max_replicas = 0;
    zero.refill_seconds = 0;
    zero.margin = 0;
    zero.floors = vec![FloorSpec::new("00:00", "24:00", 0)];
    let settings = validate_spec(&zero).unwrap();
    assert_eq!(settings.refill, Duration::ZERO);
    assert_eq!(settings.floors[0].floor, 0);
}

#[test]
fn a_window_goes_from_hh_mm_or_hh_mm_ss_and_may_end_at_24_00() {
    let mut good = spec();
    good.floors = vec![
        window("00:00", "02:00"),
        window("08:30:15", "17:45:30"),
        window("22:00", "24:00"),
    ];
    let settings = validate_spec(&good).unwrap();
    assert_eq!(settings.floors[1].from_second, 8 * 3600 + 30 * 60 + 15);
    assert_eq!(settings.floors[1].to_second, 17 * 3600 + 45 * 60 + 30);
    assert_eq!(settings.floors[2].to_second, 86_400);
}

#[test]
fn a_time_that_is_not_in_the_form_is_refused_and_the_field_is_named() {
    for text in ["9:00", "25:00", "10:61", "10", "ten", "24:00:01", "10:00 "] {
        let mut from = spec();
        from.floors = vec![window(text, "24:00")];
        assert_eq!(refused(&from).field, "floors[0].from", "{text}");
        let mut to = spec();
        to.floors = vec![window("00:00", text)];
        assert_eq!(refused(&to).field, "floors[0].to", "{text}");
    }
    let mut start_at_midnight = spec();
    start_at_midnight.floors = vec![window("24:00", "24:00")];
    assert_eq!(refused(&start_at_midnight).field, "floors[0].from");
}

#[test]
fn a_window_whose_to_is_not_after_its_from_is_refused() {
    for (from, to) in [("09:00", "09:00"), ("22:00", "02:00"), ("10:00", "09:59")] {
        let mut bad = spec();
        bad.floors = vec![window("00:00", "01:00"), window(from, to)];
        let error = refused(&bad);
        assert_eq!(error.field, "floors[1].to", "{from} {to}");
        assert!(error.reason.contains("not after"), "{error}");
    }
}

#[test]
fn at_most_16_windows_are_allowed() {
    let mut sixteen = spec();
    sixteen.floors = (0..16).map(|_| window("00:00", "01:00")).collect();
    assert!(validate_spec(&sixteen).is_ok());
    let mut seventeen = spec();
    seventeen.floors = (0..17).map(|_| window("00:00", "01:00")).collect();
    assert_eq!(refused(&seventeen).field, "floors");
}

#[test]
fn from_and_to_are_at_most_64_characters() {
    let long = "0".repeat(65);
    let mut from = spec();
    from.floors = vec![window(&long, "01:00")];
    let error = refused(&from);
    assert_eq!(error.field, "floors[0].from");
    assert!(error.reason.contains("64"), "{error}");
    let mut to = spec();
    to.floors = vec![window("00:00", &long)];
    assert_eq!(refused(&to).field, "floors[0].to");
    let edge = "0".repeat(64);
    let mut at_bound = spec();
    at_bound.floors = vec![window(&edge, "01:00")];
    let error = refused(&at_bound);
    assert!(!error.reason.contains("longer"), "{error}");
}

fn resource(spec: serde_json::Value) -> SandboxPool {
    SandboxPool {
        metadata: ObjectMeta::default(),
        spec,
        status: None,
    }
}

fn spec_json() -> serde_json::Value {
    serde_json::to_value(spec()).unwrap()
}

#[test]
fn a_resource_with_a_good_spec_is_validated_from_the_object() {
    assert!(validate(&resource(spec_json())).is_ok());
}

#[test]
fn an_unknown_key_at_each_level_is_refused_as_an_invalid_spec_and_never_dropped() {
    let mut top = spec_json();
    top["typo"] = json!(1);
    let mut size = spec_json();
    size["size"]["cpu"]["extra"] = json!(1);
    let mut floor = spec_json();
    floor["floors"] = json!([{"from": "00:00", "to": "01:00", "floor": 1, "colour": "red"}]);
    for value in [top, size, floor] {
        let error = validate(&resource(value)).unwrap_err();
        assert_eq!(error.field, "spec");
        assert!(error.reason.contains("unknown field"), "{error}");
    }
}

#[test]
fn a_missing_required_field_and_a_negative_or_text_number_are_invalid_specs() {
    for key in ["image", "size", "minReplicas", "maxReplicas"] {
        let mut value = spec_json();
        value.as_object_mut().unwrap().remove(key);
        assert_eq!(
            validate(&resource(value)).unwrap_err().field,
            "spec",
            "{key}"
        );
    }
    for bad in [json!(-1), json!("3"), json!(1.5)] {
        let mut value = spec_json();
        value["maxReplicas"] = bad;
        assert_eq!(validate(&resource(value)).unwrap_err().field, "spec");
    }
}

#[test]
fn the_message_of_a_fault_names_the_field_and_the_reason() {
    let mut bad = spec();
    bad.min_replicas = 30;
    assert_eq!(
        refused(&bad).to_string(),
        "minReplicas: it is above maxReplicas"
    );
}
