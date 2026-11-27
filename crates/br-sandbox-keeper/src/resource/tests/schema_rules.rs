use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::{
    JSONSchemaProps, JSONSchemaPropsOrArray,
};

use super::super::{MAX_FLOORS, MAX_TIME_LENGTH, definition};
use super::{at_path, spec_schema, status_schema};

const PRESERVING_LEVELS: [&[&str]; 6] = [
    &[],
    &["size"],
    &["size", "cpu"],
    &["size", "memory"],
    &["size", "disk"],
    &["floors", "[]"],
];

fn walk<'a>(
    schema: &'a JSONSchemaProps,
    path: String,
    out: &mut Vec<(String, &'a JSONSchemaProps)>,
) {
    out.push((path.clone(), schema));
    for (name, property) in schema.properties.iter().flatten() {
        walk(property, format!("{path}.{name}"), out);
    }
    if let Some(JSONSchemaPropsOrArray::Schema(items)) = &schema.items {
        walk(items, format!("{path}[]"), out);
    }
}

fn all_levels(schema: &JSONSchemaProps) -> Vec<(String, &JSONSchemaProps)> {
    let mut out = Vec::new();
    walk(schema, String::new(), &mut out);
    out
}

#[test]
fn every_object_level_of_the_spec_keeps_unknown_fields() {
    let spec = spec_schema();
    for path in PRESERVING_LEVELS {
        let level = at_path(&spec, path);
        assert_eq!(level.type_.as_deref(), Some("object"), "{path:?}");
        assert_eq!(
            level.x_kubernetes_preserve_unknown_fields,
            Some(true),
            "{path:?}"
        );
    }
}

#[test]
fn only_the_object_levels_of_the_spec_and_the_status_carry_the_mark() {
    let marked = |schema: &JSONSchemaProps| -> Vec<String> {
        all_levels(schema)
            .into_iter()
            .filter(|(_, level)| level.x_kubernetes_preserve_unknown_fields == Some(true))
            .map(|(path, _)| path)
            .collect()
    };
    assert_eq!(
        marked(&spec_schema()),
        vec![
            "",
            ".floors[]",
            ".size",
            ".size.cpu",
            ".size.disk",
            ".size.memory"
        ]
    );
    assert_eq!(marked(&status_schema()), vec![""]);
    let root = definition().spec.versions[0]
        .schema
        .clone()
        .unwrap()
        .open_api_v3_schema
        .unwrap();
    assert_eq!(root.x_kubernetes_preserve_unknown_fields, None);
}

#[test]
fn the_floors_are_bounded_at_sixteen_windows_and_sixty_four_characters() {
    let spec = spec_schema();
    let floors = at_path(&spec, &["floors"]);
    assert_eq!(MAX_FLOORS, 16);
    assert_eq!(floors.max_items, Some(16));
    assert_eq!(MAX_TIME_LENGTH, 64);
    for field in ["from", "to"] {
        let level = at_path(&spec, &["floors", "[]", field]);
        assert_eq!(level.type_.as_deref(), Some("string"), "{field}");
        assert_eq!(level.max_length, Some(64), "{field}");
    }
    assert_eq!(at_path(&spec, &["floors", "[]", "floor"]).max_length, None);
}

#[test]
fn the_definition_holds_the_four_proved_rules_and_no_other() {
    let root = definition().spec.versions[0]
        .schema
        .clone()
        .unwrap()
        .open_api_v3_schema
        .unwrap();
    let rules: Vec<(String, String, Option<String>)> = all_levels(&root)
        .into_iter()
        .flat_map(|(path, level)| {
            level
                .x_kubernetes_validations
                .iter()
                .flatten()
                .map(|rule| (path.clone(), rule.rule.clone(), rule.message.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    let expected = [
        (
            "!has(self.minReplicas) || !has(self.maxReplicas) || self.minReplicas <= self.maxReplicas",
            "minReplicas is above maxReplicas",
        ),
        ("has(self.image) && self.image != ''", "image is empty"),
        (
            "!has(oldSelf.image) || self.image == oldSelf.image || self.image != ''",
            "image cannot change to empty",
        ),
        (
            "!has(self.floors) || self.floors.all(f, !has(f.from) || !has(f.to) || f.from != f.to)",
            "a floor has from equal to to",
        ),
    ];
    let expected: Vec<(String, String, Option<String>)> = expected
        .iter()
        .map(|(rule, message)| {
            (
                ".spec".to_owned(),
                (*rule).to_owned(),
                Some((*message).to_owned()),
            )
        })
        .collect();
    assert_eq!(rules, expected);
}

#[test]
fn no_rule_of_the_definition_compares_a_request_to_a_limit() {
    let root = definition().spec.versions[0]
        .schema
        .clone()
        .unwrap()
        .open_api_v3_schema
        .unwrap();
    for (_, level) in all_levels(&root) {
        for rule in level.x_kubernetes_validations.iter().flatten() {
            assert!(!rule.rule.contains("request"), "{}", rule.rule);
            assert!(!rule.rule.contains("limit"), "{}", rule.rule);
            assert!(!rule.rule.contains("size"), "{}", rule.rule);
        }
    }
}

#[test]
fn the_schema_requires_the_image_and_bounds_the_replicas_at_zero() {
    let spec = spec_schema();
    assert_eq!(spec.required, Some(vec!["image".to_owned()]));
    for field in ["minReplicas", "maxReplicas"] {
        let level = at_path(&spec, &[field]);
        assert_eq!(level.type_.as_deref(), Some("integer"), "{field}");
        assert_eq!(level.minimum, Some(0.0), "{field}");
    }
    for field in ["refillSeconds", "margin"] {
        let level = at_path(&spec, &[field]);
        assert_eq!(level.type_.as_deref(), Some("integer"), "{field}");
        assert_eq!(level.minimum, None, "{field}");
    }
}

#[test]
fn the_size_quantities_are_strings() {
    let spec = spec_schema();
    for resource in ["cpu", "memory", "disk"] {
        for bound in ["request", "limit"] {
            let level = at_path(&spec, &["size", resource, bound]);
            assert_eq!(level.type_.as_deref(), Some("string"), "{resource} {bound}");
        }
    }
}
