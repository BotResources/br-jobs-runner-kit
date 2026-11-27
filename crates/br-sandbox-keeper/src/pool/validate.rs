use std::time::Duration;

use super::quantity::{self, Nano, Refusal};
use super::settings::{FloorWindow, PoolSettings, PoolSize, ResourceBounds};
use super::time_of_day::{self, End};
use crate::resource::{
    FloorSpec, MAX_FLOORS, MAX_TIME_LENGTH, SandboxPool, SandboxPoolSpec, SizeBounds,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {reason}")]
#[non_exhaustive]
pub struct InvalidSpec {
    pub field: String,
    pub reason: String,
}

impl InvalidSpec {
    fn new(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

fn quantity_of(field: &str, text: &str) -> Result<Nano, InvalidSpec> {
    let value = quantity::parse(text).map_err(|refusal| {
        let reason = match refusal {
            Refusal::NotAQuantity => "it is not a Kubernetes quantity",
            Refusal::Negative => "it must be above zero",
            Refusal::TooLarge => "it is too large",
        };
        InvalidSpec::new(field, reason)
    })?;
    if value.0 == 0 {
        return Err(InvalidSpec::new(field, "it must be above zero"));
    }
    Ok(value)
}

fn bounds(name: &str, spec: &SizeBounds) -> Result<ResourceBounds, InvalidSpec> {
    let request_field = format!("size.{name}.request");
    let limit_field = format!("size.{name}.limit");
    let request = quantity_of(&request_field, &spec.request.0)?;
    let limit = quantity_of(&limit_field, &spec.limit.0)?;
    if request > limit {
        return Err(InvalidSpec::new(
            request_field,
            "the request is above the limit",
        ));
    }
    Ok(ResourceBounds {
        request: spec.request.clone(),
        limit: spec.limit.clone(),
    })
}

fn window(index: usize, spec: &FloorSpec) -> Result<FloorWindow, InvalidSpec> {
    let field = |name: &str| format!("floors[{index}].{name}");
    for (name, text) in [("from", &spec.from), ("to", &spec.to)] {
        if text.chars().count() > MAX_TIME_LENGTH {
            return Err(InvalidSpec::new(
                field(name),
                format!("it is longer than {MAX_TIME_LENGTH} characters"),
            ));
        }
    }
    let from = time_of_day::parse(&spec.from, End::Start)
        .map_err(|reason| InvalidSpec::new(field("from"), reason))?;
    let to = time_of_day::parse(&spec.to, End::End)
        .map_err(|reason| InvalidSpec::new(field("to"), reason))?;
    if to <= from {
        return Err(InvalidSpec::new(
            field("to"),
            "it is not after from; a window that crosses midnight is two windows",
        ));
    }
    Ok(FloorWindow::new(from, to, spec.floor))
}

fn floors(specs: &[FloorSpec]) -> Result<Vec<FloorWindow>, InvalidSpec> {
    if specs.len() > MAX_FLOORS {
        return Err(InvalidSpec::new(
            "floors",
            format!("there are more than {MAX_FLOORS} windows"),
        ));
    }
    specs
        .iter()
        .enumerate()
        .map(|(index, spec)| window(index, spec))
        .collect()
}

pub fn validate_spec(spec: &SandboxPoolSpec) -> Result<PoolSettings, InvalidSpec> {
    if spec.image.trim().is_empty() {
        return Err(InvalidSpec::new("image", "it is blank"));
    }
    let size = PoolSize::new(
        bounds("cpu", &spec.size.cpu)?,
        bounds("memory", &spec.size.memory)?,
        bounds("disk", &spec.size.disk)?,
    );
    if spec.min_replicas > spec.max_replicas {
        return Err(InvalidSpec::new(
            "minReplicas",
            "it is above maxReplicas",
        ));
    }
    Ok(PoolSettings {
        image: spec.image.clone(),
        size,
        min_replicas: spec.min_replicas,
        max_replicas: spec.max_replicas,
        refill: Duration::from_secs(u64::from(spec.refill_seconds)),
        margin: spec.margin,
        floors: floors(&spec.floors)?,
    })
}

pub fn validate(pool: &SandboxPool) -> Result<PoolSettings, InvalidSpec> {
    let spec = pool
        .read_spec()
        .map_err(|error| InvalidSpec::new("spec", error.to_string()))?;
    validate_spec(&spec)
}

#[cfg(test)]
mod tests;
