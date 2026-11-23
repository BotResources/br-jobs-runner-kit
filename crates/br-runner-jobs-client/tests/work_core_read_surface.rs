use std::sync::Arc;

use br_runner_jobs_client::{MailHandle, WorkCore, WorkIds};
use uuid::Uuid;

#[test]
fn given_the_work_core_when_its_public_calls_are_named_then_they_are_exactly_ids_run_id_and_mail_handle()
 {
    let ids: fn(&WorkCore) -> WorkIds = WorkCore::ids;
    let run_id: fn(&WorkCore) -> Uuid = WorkCore::run_id;
    let mail_handle: fn(&Arc<WorkCore>) -> MailHandle = WorkCore::mail_handle;
    let _ = (ids, run_id, mail_handle);
}

#[test]
fn given_the_work_core_when_it_is_listed_then_the_three_read_calls_are_its_whole_public_api() {
    let listing = std::process::Command::new("cargo")
        .args(["public-api", "--version"])
        .output();
    if !matches!(listing, Ok(ref out) if out.status.success()) {
        return;
    }
    let manifest = env!("CARGO_MANIFEST_DIR");
    let out = std::process::Command::new("cargo")
        .args([
            "public-api",
            "--manifest-path",
            &format!("{manifest}/Cargo.toml"),
            "--simplified",
        ])
        .output()
        .expect("cargo public-api runs");
    if !out.status.success() {
        return;
    }
    let text = String::from_utf8(out.stdout).expect("utf8");
    let mut names: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("pub ") && line.contains("::WorkCore::"))
        .filter_map(|line| line.split("::WorkCore::").nth(1))
        .map(|tail| tail.split(['(', ':']).next().unwrap_or(tail))
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["ids", "mail_handle", "run_id"], "{text}");
}
