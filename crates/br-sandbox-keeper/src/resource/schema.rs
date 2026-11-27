use std::collections::BTreeMap;

use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::{
    JSONSchemaProps, JSONSchemaPropsOrArray, ValidationRule,
};

use super::{MAX_FLOORS, MAX_TIME_LENGTH};

fn of_type(name: &str) -> JSONSchemaProps {
    JSONSchemaProps {
        type_: Some(name.to_owned()),
        ..JSONSchemaProps::default()
    }
}

fn string() -> JSONSchemaProps {
    of_type("string")
}

fn integer() -> JSONSchemaProps {
    of_type("integer")
}

fn count() -> JSONSchemaProps {
    JSONSchemaProps {
        minimum: Some(0.0),
        ..integer()
    }
}

fn time_text() -> JSONSchemaProps {
    JSONSchemaProps {
        max_length: Some(MAX_TIME_LENGTH as i64),
        ..string()
    }
}

fn object<const N: usize>(properties: [(&str, JSONSchemaProps); N]) -> JSONSchemaProps {
    JSONSchemaProps {
        properties: Some(
            properties
                .into_iter()
                .map(|(name, schema)| (name.to_owned(), schema))
                .collect::<BTreeMap<_, _>>(),
        ),
        ..of_type("object")
    }
}

fn preserving(schema: JSONSchemaProps) -> JSONSchemaProps {
    JSONSchemaProps {
        x_kubernetes_preserve_unknown_fields: Some(true),
        ..schema
    }
}

fn array(items: JSONSchemaProps) -> JSONSchemaProps {
    JSONSchemaProps {
        items: Some(JSONSchemaPropsOrArray::Schema(Box::new(items))),
        ..of_type("array")
    }
}

fn rule(rule: &str, message: &str) -> ValidationRule {
    ValidationRule {
        rule: rule.to_owned(),
        message: Some(message.to_owned()),
        ..ValidationRule::default()
    }
}

fn size_bounds() -> JSONSchemaProps {
    preserving(object([("request", string()), ("limit", string())]))
}

fn floor_window() -> JSONSchemaProps {
    preserving(object([
        ("from", time_text()),
        ("to", time_text()),
        ("floor", integer()),
    ]))
}

pub(super) fn spec_schema() -> JSONSchemaProps {
    let size = preserving(object([
        ("cpu", size_bounds()),
        ("memory", size_bounds()),
        ("disk", size_bounds()),
    ]));
    let floors = JSONSchemaProps {
        max_items: Some(MAX_FLOORS as i64),
        ..array(floor_window())
    };
    let properties = object([
        ("image", string()),
        ("size", size),
        ("minReplicas", count()),
        ("maxReplicas", count()),
        ("refillSeconds", integer()),
        ("margin", integer()),
        ("floors", floors),
    ]);
    JSONSchemaProps {
        required: Some(vec!["image".to_owned()]),
        x_kubernetes_validations: Some(vec![
            rule(
                "!has(self.minReplicas) || !has(self.maxReplicas) || self.minReplicas <= self.maxReplicas",
                "minReplicas is above maxReplicas",
            ),
            rule("has(self.image) && self.image != ''", "image is empty"),
            rule(
                "!has(oldSelf.image) || self.image == oldSelf.image || self.image != ''",
                "image cannot change to empty",
            ),
            rule(
                "!has(self.floors) || self.floors.all(f, !has(f.from) || !has(f.to) || f.from != f.to)",
                "a floor has from equal to to",
            ),
        ]),
        ..preserving(properties)
    }
}

pub(super) fn status_schema() -> JSONSchemaProps {
    let condition = object([
        ("type", string()),
        ("status", string()),
        ("reason", string()),
        ("message", string()),
        ("lastTransitionTime", date_time()),
        ("observedGeneration", integer()),
    ]);
    preserving(object([
        ("observedGeneration", integer()),
        ("conditions", array(condition)),
        ("desiredReplicas", integer()),
        ("replicas", integer()),
        ("readyReplicas", integer()),
        ("updatedReplicas", integer()),
        ("observedAt", date_time()),
    ]))
}

fn date_time() -> JSONSchemaProps {
    JSONSchemaProps {
        format: Some("date-time".to_owned()),
        ..string()
    }
}

pub(super) fn root_schema() -> JSONSchemaProps {
    object([("spec", spec_schema()), ("status", status_schema())])
}
