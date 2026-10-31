mod calls;
mod closings;
mod context;
mod drain;
mod error;
mod exec_observer;
mod failure;
mod feeds;
mod hook;
mod hooks;
#[cfg(feature = "test-host")]
mod host;
#[cfg(feature = "test-host")]
mod host_state;
mod instances;
mod observer;
mod panic;
mod plan;
mod presence;
mod record;
mod run;
mod run_start;
mod runner;
mod saved_state;
mod settings;
mod shared;
mod signals;
mod slot;
mod supervise;
mod tap;

pub use context::RunContext;
pub use drain::{DrainReport, DrainedRun};
pub use error::{Error, ErrorCode};
pub use failure::{FailureMapping, FailureOverride, ReportedFailure, RunRefusal, failure_mapping};
pub use hook::{InstanceEnded, LifecycleHook, lifecycle_hook};
#[cfg(feature = "test-host")]
pub use host::{HostCancel, HostRun, TestHost};
pub use instances::InstanceLookup;
pub use plan::AnswerFn;
pub use run_start::{RunStart, RunStartInput, StartedRun, run_start};
pub use runner::{RunnerBuilder, StartArgs};
pub use settings::{Settings, SettingsBuilder};
#[cfg(feature = "test-host")]
pub use tap::HostEvent;
