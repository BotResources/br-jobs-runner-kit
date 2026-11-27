use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use serde::{Deserialize, Serialize};

use super::{DEFAULT_MARGIN, DEFAULT_REFILL_SECONDS};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[non_exhaustive]
pub struct SandboxPoolSpec {
    pub image: String,
    pub size: PoolSize,
    pub min_replicas: u32,
    pub max_replicas: u32,
    #[serde(default = "default_refill_seconds")]
    pub refill_seconds: u32,
    #[serde(default = "default_margin")]
    pub margin: u32,
    #[serde(default)]
    pub floors: Vec<FloorSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PoolSize {
    pub cpu: SizeBounds,
    pub memory: SizeBounds,
    pub disk: SizeBounds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct SizeBounds {
    pub request: Quantity,
    pub limit: Quantity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct FloorSpec {
    pub from: String,
    pub to: String,
    pub floor: u32,
}

fn default_refill_seconds() -> u32 {
    DEFAULT_REFILL_SECONDS
}

fn default_margin() -> u32 {
    DEFAULT_MARGIN
}

impl SandboxPoolSpec {
    pub fn new(
        image: impl Into<String>,
        size: PoolSize,
        min_replicas: u32,
        max_replicas: u32,
    ) -> Self {
        Self {
            image: image.into(),
            size,
            min_replicas,
            max_replicas,
            refill_seconds: DEFAULT_REFILL_SECONDS,
            margin: DEFAULT_MARGIN,
            floors: Vec::new(),
        }
    }
}

impl PoolSize {
    pub fn new(cpu: SizeBounds, memory: SizeBounds, disk: SizeBounds) -> Self {
        Self { cpu, memory, disk }
    }
}

impl SizeBounds {
    pub fn new(request: impl Into<String>, limit: impl Into<String>) -> Self {
        Self {
            request: Quantity(request.into()),
            limit: Quantity(limit.into()),
        }
    }
}

impl FloorSpec {
    pub fn new(from: impl Into<String>, to: impl Into<String>, floor: u32) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            floor,
        }
    }
}
