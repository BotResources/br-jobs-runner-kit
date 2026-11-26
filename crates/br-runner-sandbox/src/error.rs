use br_agentic_harness_contract::{AlreadySaved, OpenFault, PortError};

use crate::cluster::ClusterError;
use crate::config::ConfigError;
use crate::decide::WebAccessMismatch;
use crate::exec::ExecError;
use crate::fault::ImageError;
use crate::formats::FormatError;
use crate::seed::SeedError;
use crate::store::StoreError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Format(#[from] FormatError),
    #[error(transparent)]
    Seed(#[from] SeedError),
    #[error(transparent)]
    Mode(#[from] WebAccessMismatch),
    #[error(transparent)]
    Image(#[from] ImageError),
    #[error(transparent)]
    Cluster(#[from] ClusterError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Exec(#[from] ExecError),
    #[error("a save of this thread, this agent and this turn already exists")]
    AlreadySaved,
}

impl From<Error> for PortError {
    fn from(error: Error) -> Self {
        match error {
            Error::Image(image) => PortError::from(OpenFault::from(&image)),
            Error::Mode(mismatch) => PortError::from(mismatch),
            Error::AlreadySaved => AlreadySaved::refusal(),
            Error::Seed(seed) if seed.is_refusal() => PortError::refused(seed.to_string()),
            Error::Exec(exec) if exec.is_refusal() => PortError::refused(exec.to_string()),
            other => PortError::fault(other),
        }
    }
}
