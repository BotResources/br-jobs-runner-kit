use serde::Serialize;
use serde::de::DeserializeOwned;

use super::error::FormatError;

pub(super) fn decode<T: DeserializeOwned>(
    object: &'static str,
    bytes: &[u8],
) -> Result<T, FormatError> {
    serde_json::from_slice(bytes).map_err(|error| FormatError::Malformed {
        object,
        reason: error.to_string(),
    })
}

pub(super) fn encode<T: Serialize>(
    object: &'static str,
    value: &T,
) -> Result<Vec<u8>, FormatError> {
    serde_json::to_vec(value).map_err(|error| FormatError::Malformed {
        object,
        reason: error.to_string(),
    })
}
