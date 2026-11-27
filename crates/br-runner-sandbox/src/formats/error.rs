use crate::ids::InvalidAttemptId;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    #[error("the {object} is not valid JSON of its format: {reason}")]
    Malformed {
        object: &'static str,
        reason: String,
    },
    #[error("the {object} has the field {field} with a value that is not valid")]
    Field {
        object: &'static str,
        field: &'static str,
    },
    #[error("the {object} is not the one of the save key that was asked")]
    OtherKey { object: &'static str },
    #[error("the part has {found} stored bytes and the manifest names {expected}")]
    StoredSize { expected: u64, found: u64 },
    #[error("the part has more than the {limit} raw bytes that the manifest names")]
    RawSize { limit: u64 },
    #[error("the part holds {found} raw bytes and the manifest names {expected}")]
    RawLength { expected: u64, found: u64 },
    #[error("the {which} hash of the part is not the one of the manifest")]
    Hash { which: &'static str },
    #[error("the zstd stream of the part is not valid: {0}")]
    Compression(String),
    #[error("the zstd level {0} is not valid")]
    Level(i32),
}

impl From<InvalidAttemptId> for FormatError {
    fn from(_: InvalidAttemptId) -> Self {
        Self::Field {
            object: "manifest",
            field: "attempt",
        }
    }
}
