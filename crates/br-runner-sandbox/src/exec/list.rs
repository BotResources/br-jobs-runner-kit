use br_agentic_harness_contract::FilePath;

use super::argv::WORK_DIR;
use super::error::ExecError;

const FIND_PARTIAL_EXIT: i32 = 1;

pub fn parse_find_output(code: i32, stdout: &[u8]) -> Result<Vec<FilePath>, ExecError> {
    if code != 0 && code != FIND_PARTIAL_EXIT {
        return Err(ExecError::Failed {
            operation: "list",
            code,
        });
    }
    let prefix = format!("{WORK_DIR}/");
    let mut paths: Vec<FilePath> = stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .filter_map(|name| std::str::from_utf8(name).ok())
        .filter_map(|name| name.strip_prefix(prefix.as_str()))
        .filter_map(|relative| FilePath::new(relative).ok())
        .collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}
