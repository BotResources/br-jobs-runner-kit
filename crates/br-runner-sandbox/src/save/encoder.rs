use std::io::Write;

use sha2::{Digest, Sha256};
use zstd::stream::write::Encoder;

use crate::formats::{FormatError, PartReport, Sha256Hex};

use super::tar_count::{TarCounter, TarError};

pub struct PartEncoder {
    encoder: Encoder<'static, Vec<u8>>,
    raw: Sha256,
    stored: Sha256,
    bytes_raw: u64,
    bytes_stored: u64,
    counter: TarCounter,
}

fn tar_error(error: TarError) -> FormatError {
    FormatError::Compression(error.to_string())
}

fn io_error(error: std::io::Error) -> FormatError {
    FormatError::Compression(error.to_string())
}

impl PartEncoder {
    pub fn new(level: i32) -> Result<Self, FormatError> {
        if !(1..=22).contains(&level) {
            return Err(FormatError::Level(level));
        }
        Ok(Self {
            encoder: Encoder::new(Vec::new(), level).map_err(io_error)?,
            raw: Sha256::new(),
            stored: Sha256::new(),
            bytes_raw: 0,
            bytes_stored: 0,
            counter: TarCounter::new(),
        })
    }

    pub fn push(&mut self, raw: &[u8]) -> Result<Vec<u8>, FormatError> {
        self.counter.push(raw).map_err(tar_error)?;
        self.raw.update(raw);
        self.bytes_raw += raw.len() as u64;
        self.encoder.write_all(raw).map_err(io_error)?;
        let stored = std::mem::take(self.encoder.get_mut());
        Ok(self.account(stored))
    }

    pub fn finish(self) -> Result<(Vec<u8>, PartReport), FormatError> {
        let Self {
            encoder,
            raw,
            mut stored,
            bytes_raw,
            mut bytes_stored,
            counter,
        } = self;
        let tail = encoder.finish().map_err(io_error)?;
        stored.update(&tail);
        bytes_stored += tail.len() as u64;
        let report = PartReport {
            entries: counter.finish().map_err(tar_error)?,
            bytes_raw,
            bytes_stored,
            sha256_raw: Sha256Hex::from_bytes(raw.finalize().into()),
            sha256_stored: Sha256Hex::from_bytes(stored.finalize().into()),
        };
        Ok((tail, report))
    }

    fn account(&mut self, stored: Vec<u8>) -> Vec<u8> {
        self.stored.update(&stored);
        self.bytes_stored += stored.len() as u64;
        stored
    }
}
