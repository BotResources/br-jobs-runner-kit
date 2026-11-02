mod delivery;
mod fields;
mod own_calls;
mod queue_once;
mod started;
mod text_forms;
mod text_forms_errors;
mod text_forms_token;
mod unreadable;

use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use serde_json::{Value, json};
use uuid::Uuid;

use super::Work;
use crate::queue::{IgnoreReports, QueueSettings};
use crate::test_double::{ScriptedJobs, trigger_json, work_from_trigger};

pub(super) const JOB: u128 = 1;
pub(super) const RUN: u128 = 2;
pub(super) const PARENT: u128 = 3;
pub(super) const INITIAL: u128 = 4;
pub(super) const USER: u128 = 5;
pub(super) const IMPERSONATOR: u128 = 6;
pub(super) const START: u128 = 7;

pub(super) fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

pub(super) fn passport() -> Passport {
    let claims = PassportClaims::from_value(
        json!({ "department": "claim-value-ocelot", "email": "ocelot@example.test" }),
    )
    .expect("claims");
    Passport::human(
        id(USER),
        false,
        true,
        AuthMethod::Jwt,
        Some(id(IMPERSONATOR)),
        claims,
    )
}

pub(super) fn trigger() -> Value {
    trigger_json(id(RUN), id(JOB), id(PARENT), id(INITIAL), &passport()).expect("a trigger")
}

pub(super) fn trigger_with(fields: &[(&str, Value)]) -> Value {
    let mut value = trigger();
    let object = value.as_object_mut().expect("an object");
    for (key, field) in fields {
        object.insert((*key).to_owned(), field.clone());
    }
    value
}

pub(super) fn trigger_without(key: &str) -> Value {
    let mut value = trigger();
    value.as_object_mut().expect("an object").remove(key);
    value
}

pub(super) fn jobs() -> Arc<ScriptedJobs> {
    ScriptedJobs::new(
        crate::RunnerType::new("runner").expect("a runner type"),
        "instance-a",
    )
}

pub(super) fn work_of(jobs: &Arc<ScriptedJobs>, value: Value) -> Work {
    work_from_trigger(jobs, value)
        .map_err(|unreadable| unreadable.error.to_string())
        .expect("a readable trigger")
}

pub(super) fn work(jobs: &Arc<ScriptedJobs>) -> Work {
    work_of(jobs, trigger())
}

pub(super) fn settings() -> QueueSettings {
    QueueSettings {
        start_id: id(START),
        outcome_wait: Duration::from_secs(5),
        retry_wait: Duration::from_millis(10),
        observer: Arc::new(IgnoreReports),
    }
}

#[allow(dead_code)] // the stand-in is shared with the tests of the connection crate and each file uses part of it
#[path = "../../../../br-runner-connection/tests/support/server.rs"]
pub(super) mod http_server;
