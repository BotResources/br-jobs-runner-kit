use std::time::Duration;

use k8s_openapi::apimachinery::pkg::api::resource::Quantity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FloorWindow {
    pub from_second: u32,
    pub to_second: u32,
    pub floor: u32,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResourceBounds {
    pub request: Quantity,
    pub limit: Quantity,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PoolSize {
    pub cpu: ResourceBounds,
    pub memory: ResourceBounds,
    pub disk: ResourceBounds,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PoolSettings {
    pub image: String,
    pub size: PoolSize,
    pub min_replicas: u32,
    pub max_replicas: u32,
    pub refill: Duration,
    pub margin: u32,
    pub floors: Vec<FloorWindow>,
}

impl FloorWindow {
    pub fn new(from_second: u32, to_second: u32, floor: u32) -> Self {
        Self {
            from_second,
            to_second,
            floor,
        }
    }
}

impl ResourceBounds {
    pub fn new(request: impl Into<String>, limit: impl Into<String>) -> Self {
        Self {
            request: Quantity(request.into()),
            limit: Quantity(limit.into()),
        }
    }
}

impl PoolSize {
    pub fn new(cpu: ResourceBounds, memory: ResourceBounds, disk: ResourceBounds) -> Self {
        Self { cpu, memory, disk }
    }
}
