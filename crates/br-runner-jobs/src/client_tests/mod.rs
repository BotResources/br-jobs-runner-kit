mod claims;
mod feeds;
mod made;
mod presence;

use std::sync::Arc;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::client::JobsClient;
use crate::test_double::{ScriptedJobs, trigger_json};

pub(super) fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

pub(super) fn trigger() -> Value {
    let passport = Passport::human(
        id(5),
        false,
        true,
        AuthMethod::Jwt,
        None,
        PassportClaims::new(),
    );
    trigger_json(id(2), id(1), id(3), id(4), &passport).expect("a trigger")
}

pub(super) fn scripted() -> (Arc<ScriptedJobs>, JobsClient) {
    let jobs = ScriptedJobs::new("analyst", "pod-1");
    let client = jobs.client();
    (jobs, client)
}

pub(super) fn reply(run: Option<Value>) -> Vec<u8> {
    let mut value = json!({ "version": 2 });
    if let Some(run) = run {
        value["run"] = run;
    }
    serde_json::to_vec(&value).expect("bytes")
}
