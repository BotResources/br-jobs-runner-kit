#![doc = include_str!("../README.md")]

mod config;
mod context;
mod error;
mod facts;
mod run_loop;
mod runner;
mod transport;

use std::future::Future;

pub use br_jobs_runner_core as core;
pub use config::{InstanceKey, RunnerConfig, RunnerType};
pub use context::{LOG_LEVEL_ERROR, LOG_LEVEL_INFO, LOG_LEVEL_WARNING, RunContext};
pub use contract_jobs as contract;
pub use error::HarnessError;
pub use facts::RunFact;
pub use runner::{FailureReport, RunOutcome, Runner};

pub async fn run<R: Runner>(config: RunnerConfig, runner: R) -> Result<(), HarnessError> {
    run_until(config, runner, shutdown_signal()).await
}

pub async fn run_until<R, S>(
    config: RunnerConfig,
    runner: R,
    shutdown: S,
) -> Result<(), HarnessError>
where
    R: Runner,
    S: Future<Output = ()> + Send,
{
    run_loop::run_loop(config, runner, shutdown).await
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = ctrl_c => {}
                    _ = terminate.recv() => {}
                }
            }
            Err(error) => {
                tracing::warn!(%error, "SIGTERM handler unavailable; draining on ctrl-c only");
                let _ = ctrl_c.await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}
