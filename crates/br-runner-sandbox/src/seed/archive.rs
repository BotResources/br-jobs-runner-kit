use br_agentic_harness_contract::{Seed, SeedKind};
use tar::{Builder, EntryType, Header};

use super::error::SeedError;
use super::path::check_seed_path;

pub const SEED_UID_GID: u64 = 0;
const WRITE_BITS: u32 = 0o222;
const MODE_BITS: u32 = 0o7777;

pub fn seed_mode(mode: u32) -> u32 {
    mode & MODE_BITS & !WRITE_BITS
}

pub fn seed_hash_text(seed: &Seed) -> String {
    seed.content_hash().to_hex()
}

fn archive_error(error: std::io::Error) -> SeedError {
    SeedError::Archive(error.to_string())
}

fn header(kind: EntryType, mode: u32, size: u64) -> Header {
    let mut header = Header::new_gnu();
    header.set_entry_type(kind);
    header.set_mode(seed_mode(mode));
    header.set_uid(SEED_UID_GID);
    header.set_gid(SEED_UID_GID);
    header.set_mtime(0);
    header.set_size(size);
    header
}

pub fn seed_archive(seed: &Seed) -> Result<Option<Vec<u8>>, SeedError> {
    if seed.is_empty() {
        return Ok(None);
    }
    for entry in seed.entries() {
        check_seed_path(entry.path.as_str())?;
    }
    let mut builder = Builder::new(Vec::new());
    for entry in seed.entries() {
        let path = entry.path.as_str();
        match &entry.kind {
            SeedKind::File(bytes) => {
                let mut header = header(EntryType::Regular, entry.mode, bytes.len() as u64);
                builder
                    .append_data(&mut header, path, bytes.as_slice())
                    .map_err(archive_error)?;
            }
            SeedKind::Directory => {
                let mut header = header(EntryType::Directory, entry.mode, 0);
                builder
                    .append_data(&mut header, format!("{path}/"), std::io::empty())
                    .map_err(archive_error)?;
            }
            SeedKind::Symlink(target) => {
                let mut header = header(EntryType::Symlink, entry.mode, 0);
                builder
                    .append_link(&mut header, path, target.as_str())
                    .map_err(archive_error)?;
            }
        }
    }
    builder.into_inner().map(Some).map_err(archive_error)
}
