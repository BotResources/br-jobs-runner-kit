use std::sync::Arc;

use br_core_auth::Passport;
use contract_jobs::runner::{TriggerV2, WIRE_VERSION_V2};
use serde_json::Value;
use uuid::Uuid;

use super::ScriptedJobs;
use crate::backend::Backend;
use crate::work::{Unreadable, Work};

pub fn trigger_json(
    run_id: Uuid,
    job_id: Uuid,
    parent_job_id: Uuid,
    initial_job_id: Uuid,
    passport: &Passport,
) -> Result<Value, serde_json::Error> {
    serde_json::to_value(TriggerV2 {
        version: WIRE_VERSION_V2,
        run_id,
        job_id,
        attempt: 1,
        parent_job_id,
        initial_job_id,
        passport: serde_json::to_value(passport)?,
        triggered_by: None,
        config: None,
        payload: None,
        mode: None,
        caller_address: None,
        restart: None,
    })
}

pub fn work_from_trigger(jobs: &Arc<ScriptedJobs>, trigger: Value) -> Result<Work, Unreadable> {
    Work::from_value(Arc::clone(jobs) as Arc<dyn Backend>, trigger, None)
}
