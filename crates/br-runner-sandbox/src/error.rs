use br_agentic_harness_contract::{OpenFault, PortError};

use crate::config::ConfigError;
use crate::decide::WebAccessMismatch;
use crate::fault::ImageError;
use crate::formats::FormatError;
use crate::seed::SeedError;

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
}

impl From<Error> for PortError {
    fn from(error: Error) -> Self {
        match error {
            Error::Image(image) => PortError::from(OpenFault::from(&image)),
            Error::Mode(mismatch) => PortError::from(mismatch),
            Error::Seed(seed) if seed.is_refusal() => PortError::refused(seed.to_string()),
            other => PortError::fault(other),
        }
    }
}
