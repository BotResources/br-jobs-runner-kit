use br_agentic_harness_contract::FilePath;
use tar::{Builder, EntryType, Header};

use crate::time::Timestamp;

use super::error::ExecError;

const FILE_MODE: u32 = 0o644;

fn failure(error: std::io::Error) -> ExecError {
    ExecError::io("write", error)
}

pub fn file_archive(
    path: &FilePath,
    content: &[u8],
    modified: Timestamp,
) -> Result<Vec<u8>, ExecError> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_mode(FILE_MODE);
    header.set_size(content.len() as u64);
    header.set_mtime(
        modified
            .to_system_time()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default(),
    );
    let mut builder = Builder::new(Vec::new());
    builder
        .append_data(&mut header, path.as_str(), content)
        .map_err(failure)?;
    builder.into_inner().map_err(failure)
}
