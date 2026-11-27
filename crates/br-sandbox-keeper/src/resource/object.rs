use std::borrow::Cow;

use k8s_openapi::NamespaceResourceScope;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::Resource;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::spec::SandboxPoolSpec;
use super::status::SandboxPoolStatus;
use super::{GROUP, KIND, PLURAL, SpecError, VERSION};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SandboxPool {
    pub metadata: ObjectMeta,
    #[serde(default)]
    pub spec: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<SandboxPoolStatus>,
}

impl SandboxPool {
    pub fn new(metadata: ObjectMeta, spec: &SandboxPoolSpec) -> Result<Self, SpecError> {
        let spec = serde_json::to_value(spec).map_err(|error| SpecError::new(error.to_string()))?;
        Ok(Self {
            metadata,
            spec,
            status: None,
        })
    }

    pub fn read_spec(&self) -> Result<SandboxPoolSpec, SpecError> {
        serde_json::from_value(self.spec.clone()).map_err(|error| SpecError::new(error.to_string()))
    }
}

impl Resource for SandboxPool {
    type DynamicType = ();
    type Scope = NamespaceResourceScope;

    fn kind(_: &()) -> Cow<'_, str> {
        Cow::Borrowed(KIND)
    }

    fn group(_: &()) -> Cow<'_, str> {
        Cow::Borrowed(GROUP)
    }

    fn version(_: &()) -> Cow<'_, str> {
        Cow::Borrowed(VERSION)
    }

    fn plural(_: &()) -> Cow<'_, str> {
        Cow::Borrowed(PLURAL)
    }

    fn meta(&self) -> &ObjectMeta {
        &self.metadata
    }

    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.metadata
    }
}
