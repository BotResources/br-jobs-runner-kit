use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::{
    CustomResourceColumnDefinition, CustomResourceDefinition, CustomResourceDefinitionNames,
    CustomResourceDefinitionSpec, CustomResourceDefinitionVersion, CustomResourceSubresourceStatus,
    CustomResourceSubresources, CustomResourceValidation,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

use super::schema::root_schema;
use super::{GROUP, KIND, LIST_KIND, PLURAL, SINGULAR, VERSION};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the definition cannot be written as YAML: {0}")]
#[non_exhaustive]
pub struct DefinitionError(String);

fn column(name: &str, type_: &str, json_path: &str) -> CustomResourceColumnDefinition {
    CustomResourceColumnDefinition {
        name: name.to_owned(),
        type_: type_.to_owned(),
        json_path: json_path.to_owned(),
        ..CustomResourceColumnDefinition::default()
    }
}

pub fn definition() -> CustomResourceDefinition {
    CustomResourceDefinition {
        metadata: ObjectMeta {
            name: Some(format!("{PLURAL}.{GROUP}")),
            ..ObjectMeta::default()
        },
        spec: CustomResourceDefinitionSpec {
            group: GROUP.to_owned(),
            scope: "Namespaced".to_owned(),
            names: CustomResourceDefinitionNames {
                kind: KIND.to_owned(),
                list_kind: Some(LIST_KIND.to_owned()),
                plural: PLURAL.to_owned(),
                singular: Some(SINGULAR.to_owned()),
                ..CustomResourceDefinitionNames::default()
            },
            versions: vec![CustomResourceDefinitionVersion {
                name: VERSION.to_owned(),
                served: true,
                storage: true,
                subresources: Some(CustomResourceSubresources {
                    status: Some(CustomResourceSubresourceStatus(serde_json::Value::Object(
                        serde_json::Map::new(),
                    ))),
                    scale: None,
                }),
                additional_printer_columns: Some(vec![
                    column("Desired", "integer", ".status.desiredReplicas"),
                    column("Ready", "integer", ".status.readyReplicas"),
                    column(
                        "Accepted",
                        "string",
                        ".status.conditions[?(@.type==\"Accepted\")].status",
                    ),
                    column("Age", "date", ".metadata.creationTimestamp"),
                ]),
                schema: Some(CustomResourceValidation {
                    open_api_v3_schema: Some(root_schema()),
                }),
                ..CustomResourceDefinitionVersion::default()
            }],
            ..CustomResourceDefinitionSpec::default()
        },
        ..CustomResourceDefinition::default()
    }
}

pub fn definition_yaml() -> Result<String, DefinitionError> {
    serde_saphyr::to_string(&definition()).map_err(|error| DefinitionError(error.to_string()))
}
