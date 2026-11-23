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
fn given_the_work_core_source_when_its_public_items_are_listed_then_the_three_read_calls_are_its_whole_public_api()
 {
    let source = include_str!("../src/work/core.rs");
    let mut fns: Vec<&str> = Vec::new();
    let mut fields: Vec<&str> = Vec::new();
    for line in source.lines().map(str::trim_start) {
        if let Some(rest) = line.strip_prefix("pub fn ") {
            fns.push(rest.split(['(', '<']).next().unwrap_or(rest));
        } else if let Some(rest) = line
            .strip_prefix("pub ")
            .filter(|rest| !rest.starts_with("struct "))
        {
            fields.push(rest);
        }
    }
    fns.sort_unstable();
    assert_eq!(fns, ["ids", "mail_handle", "run_id"]);
    assert!(fields.is_empty(), "{fields:?}");
}
