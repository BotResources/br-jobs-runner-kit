use super::error::SeedError;

pub fn check_seed_path(path: &str) -> Result<&str, SeedError> {
    let refused = path.is_empty()
        || path.starts_with('/')
        || path.contains('\0')
        || path.split('/').any(|part| part == "..");
    if refused {
        Err(SeedError::Path {
            path: path.to_owned(),
        })
    } else {
        Ok(path)
    }
}
