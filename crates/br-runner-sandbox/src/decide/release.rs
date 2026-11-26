#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleasePlan {
    Delete,
    KeepIdle,
}

pub fn release_plan(saved_bytes_raw: Option<u64>, idle_keep_min_state_bytes: u64) -> ReleasePlan {
    match saved_bytes_raw {
        Some(bytes) if bytes >= idle_keep_min_state_bytes => ReleasePlan::KeepIdle,
        Some(_) | None => ReleasePlan::Delete,
    }
}
