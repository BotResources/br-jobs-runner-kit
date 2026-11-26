use std::io::Write;

use sha2::{Digest, Sha256};
use zstd::stream::write::Decoder;

use crate::formats::{FormatError, ManifestPart, Sha256Hex};

pub struct PartDecoder {
    expected: ManifestPart,
    decoder: Decoder<'static, Vec<u8>>,
    raw: Sha256,
    stored: Sha256,
    bytes_raw: u64,
    bytes_stored: u64,
}

fn io_error(error: std::io::Error) -> FormatError {
    FormatError::Compression(error.to_string())
}

impl PartDecoder {
    pub fn new(part: &ManifestPart) -> Result<Self, FormatError> {
        Ok(Self {
            expected: part.clone(),
            decoder: Decoder::new(Vec::new()).map_err(io_error)?,
            raw: Sha256::new(),
            stored: Sha256::new(),
            bytes_raw: 0,
            bytes_stored: 0,
        })
    }

    pub fn push(&mut self, stored: &[u8]) -> Result<Vec<u8>, FormatError> {
        self.bytes_stored += stored.len() as u64;
        if self.bytes_stored > self.expected.bytes_stored {
            return Err(FormatError::StoredSize {
                expected: self.expected.bytes_stored,
                found: self.bytes_stored,
            });
        }
        self.stored.update(stored);
        self.decoder.write_all(stored).map_err(io_error)?;
        let raw = std::mem::take(self.decoder.get_mut());
        self.bytes_raw += raw.len() as u64;
        if self.bytes_raw > self.expected.bytes_raw {
            return Err(FormatError::RawSize {
                limit: self.expected.bytes_raw,
            });
        }
        self.raw.update(&raw);
        Ok(raw)
    }

    pub fn finish(mut self) -> Result<Vec<u8>, FormatError> {
        self.decoder.flush().map_err(io_error)?;
        let rest = std::mem::take(self.decoder.get_mut());
        self.bytes_raw += rest.len() as u64;
        self.raw.update(&rest);
        if self.bytes_stored != self.expected.bytes_stored {
            return Err(FormatError::StoredSize {
                expected: self.expected.bytes_stored,
                found: self.bytes_stored,
            });
        }
        if self.bytes_raw > self.expected.bytes_raw {
            return Err(FormatError::RawSize {
                limit: self.expected.bytes_raw,
            });
        }
        if self.bytes_raw != self.expected.bytes_raw {
            return Err(FormatError::RawLength {
                expected: self.expected.bytes_raw,
                found: self.bytes_raw,
            });
        }
        if Sha256Hex::from_bytes(self.stored.finalize().into()) != self.expected.sha256_stored {
            return Err(FormatError::Hash { which: "stored" });
        }
        if Sha256Hex::from_bytes(self.raw.finalize().into()) != self.expected.sha256_raw {
            return Err(FormatError::Hash { which: "raw" });
        }
        Ok(rest)
    }
}
