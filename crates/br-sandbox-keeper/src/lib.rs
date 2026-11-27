#![doc = include_str!("../README.md")]

pub mod bucket;
pub mod cluster;
mod config;
#[cfg(test)]
mod contract;
mod controller;
mod convert;
mod environment;
mod error;
pub mod facts;
#[cfg(test)]
mod fakes;
mod http;
mod leader;
mod metrics;
mod readiness;
pub mod resource;
pub mod rights;
pub mod rules;
mod runner;
mod sealed;
mod sweep;
#[cfg(test)]
mod testing;
mod time;
mod tls;

pub use config::{
    ConfigError, FloorInput, FloorWindow, KeeperConfig, KeeperConfigInput, LeaseSettings,
    PoolConfig, PoolInput, StoreSettings,
};
pub use environment::{
    ACCESS_KEY_VARIABLE, CONFIG_VARIABLE, POD_NAMESPACE_FILE, SECRET_KEY_VARIABLE, StoreCredential,
    config_path, own_namespace,
};
pub use error::KeeperError;
pub use leader::LEASE_NAME;
pub use runner::{Parts, run};
