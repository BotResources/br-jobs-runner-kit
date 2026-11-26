use std::time::Duration;

use br_agentic_harness_contract::{OpenFault, PortError};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImageError {
    #[error("no box was ready after {waited:?}")]
    Unavailable { waited: Duration },
    #[error("the warm pods of the pool do not start: {reason}: {message}")]
    Failing { reason: String, message: String },
}

impl ImageError {
    pub fn unavailable(waited: Duration) -> Self {
        Self::Unavailable { waited }
    }

    pub fn failing(reason: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Failing {
            reason: reason.into(),
            message: message.into(),
        }
    }

    pub fn fault(&self) -> OpenFault {
        match self {
            Self::Unavailable { .. } => OpenFault::ImageUnavailable,
            Self::Failing { .. } => OpenFault::ImageFailing,
        }
    }

    pub fn port_error(&self) -> PortError {
        PortError::from(self.fault())
    }
}

impl From<&ImageError> for OpenFault {
    fn from(error: &ImageError) -> Self {
        error.fault()
    }
}
