#![doc = include_str!("../README.md")]

mod config;
mod context;
mod facts;
mod runner;

pub use br_jobs_runner_core as core;
pub use config::{InstanceKey, KeyError, RunnerConfig, RunnerType};
pub use context::RunContext;
pub use facts::RunFact;
pub use runner::{FailureReport, RunOutcome, Runner};
