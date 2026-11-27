use br_runner_sandbox::{POOL_WARM, PodKeys, Sha256Hex};
use k8s_openapi::api::core::v1::ConfigMap;
use serde_json::{Map, Value, json};

use super::canonical::canonical;
use super::names::{POD_TEMPLATE_KEY, template_hash_key};
use super::settings::{PoolSettings, ResourceBounds};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TemplateFault {
    #[error("the ConfigMap of the pod template is absent")]
    Absent,
    #[error("the pod template is not valid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoolTemplate {
    value: Value,
    agent_index: usize,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RenderedTemplate {
    pub template: Value,
    pub hash: String,
}

fn invalid(reason: impl Into<String>) -> TemplateFault {
    TemplateFault::Invalid(reason.into())
}

fn label_of<'a>(template: &'a Value, key: &str) -> Option<&'a str> {
    template
        .pointer("/metadata/labels")?
        .as_object()?
        .get(key)?
        .as_str()
}

fn agent_position(template: &Value, agent_container: &str) -> Option<usize> {
    template
        .pointer("/spec/containers")?
        .as_array()?
        .iter()
        .position(|container| container["name"].as_str() == Some(agent_container))
}

impl PoolTemplate {
    pub fn read(
        config_map: Option<&ConfigMap>,
        namespace: &str,
        keys: &PodKeys,
        agent_container: &str,
    ) -> Result<Self, TemplateFault> {
        let config_map = config_map.ok_or(TemplateFault::Absent)?;
        let text = config_map
            .data
            .as_ref()
            .and_then(|data| data.get(POD_TEMPLATE_KEY))
            .ok_or_else(|| invalid(format!("the ConfigMap holds no key {POD_TEMPLATE_KEY}")))?;
        let value: Value = serde_json::from_str(text)
            .map_err(|error| invalid(format!("{POD_TEMPLATE_KEY} is not JSON: {error}")))?;
        if !value.is_object() {
            return Err(invalid(format!("{POD_TEMPLATE_KEY} is not a JSON object")));
        }
        let pool_label = keys.pool();
        if label_of(&value, &pool_label) != Some(POOL_WARM) {
            return Err(invalid(format!(
                "the label {pool_label} must be {POOL_WARM}"
            )));
        }
        let pool_id_label = keys.pool_id();
        if label_of(&value, &pool_id_label) != Some(namespace) {
            return Err(invalid(format!(
                "the label {pool_id_label} must be {namespace}"
            )));
        }
        let agent_index = agent_position(&value, agent_container)
            .ok_or_else(|| invalid(format!("no container is named {agent_container}")))?;
        Ok(Self { value, agent_index })
    }

    pub fn render(&self, settings: &PoolSettings, keys: &PodKeys) -> RenderedTemplate {
        let mut template = self.value.clone();
        if let Some(annotations) = template
            .pointer_mut("/metadata/annotations")
            .and_then(Value::as_object_mut)
        {
            annotations.remove(&template_hash_key(keys.prefix()));
        }
        if let Some(container) = template
            .pointer_mut(&format!("/spec/containers/{}", self.agent_index))
            .and_then(Value::as_object_mut)
        {
            container.insert("image".to_owned(), json!(settings.image));
            container.insert("resources".to_owned(), resources_of(settings));
        }
        let hash = Sha256Hex::of(canonical(&template).as_bytes()).to_string();
        if let Some(metadata) = template.get_mut("metadata").and_then(Value::as_object_mut) {
            let annotations = metadata
                .entry("annotations")
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(annotations) = annotations.as_object_mut() {
                annotations.insert(template_hash_key(keys.prefix()), json!(hash));
            }
        }
        RenderedTemplate { template, hash }
    }
}

fn resources_of(settings: &PoolSettings) -> Value {
    let pick = |side: fn(&ResourceBounds) -> &str| {
        json!({
            "cpu": side(&settings.size.cpu),
            "memory": side(&settings.size.memory),
            "ephemeral-storage": side(&settings.size.disk),
        })
    };
    json!({
        "requests": pick(|bounds| bounds.request.0.as_str()),
        "limits": pick(|bounds| bounds.limit.0.as_str()),
    })
}

#[cfg(test)]
mod tests;
