use br_runner_jobs::test_double::{Call, ScriptedJobs};
use serde_json::{Value, json};

use super::{call_with, id, trigger};

pub fn restart_trigger(n: u128) -> Vec<u8> {
    call_with(
        trigger(n),
        &[("restart", json!({ "restart_of_run_id": id(50) }))],
    )
}

pub fn writes(jobs: &ScriptedJobs) -> Vec<Value> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Graphql {
                document,
                variables,
            } if document.contains("jobsSaveState") => Some(variables),
            _ => None,
        })
        .collect()
}
