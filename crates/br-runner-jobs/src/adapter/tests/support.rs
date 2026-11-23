use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use br_runner_jobs_client::test_double::{ScriptedJobs, trigger_json, work_from_trigger};
use br_runner_jobs_client::{IgnoreReports, QueueSettings, RunnerType, Work};
use contract_jobs::graphql_roots::*;
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

fn passport() -> Passport {
    Passport::human(
        id(5),
        false,
        true,
        AuthMethod::Jwt,
        Some(id(6)),
        PassportClaims::new(),
    )
}

fn plain(root: &str) -> Value {
    json!({ root: { "success": true, "reason": null, "message": null } })
}

pub(crate) fn everything_succeeds() -> Value {
    let mut all = serde_json::Map::new();
    for root in [
        UPLOAD_PAYLOAD_ROOT,
        UPLOAD_RESULT_ROOT,
        SAVE_STATE_ROOT,
        SEND_MAIL_ROOT,
        UPLOAD_ATTACHMENT_ROOT,
    ] {
        all.extend(plain(root).as_object().cloned().unwrap_or_default());
    }
    all.insert(
        UNREAD_ROOT.to_owned(),
        json!({ "success": true, "reason": null, "message": null, "items": [] }),
    );
    Value::Object(all)
}

pub(crate) fn scripted() -> Arc<ScriptedJobs> {
    let jobs = ScriptedJobs::new(
        RunnerType::new("runner").expect("a runner type"),
        "instance-a",
    );
    jobs.graphql_default(everything_succeeds());
    jobs
}

pub(crate) fn work(jobs: &Arc<ScriptedJobs>) -> Work {
    let trigger = trigger_json(id(2), id(1), id(3), id(4), &passport()).expect("a trigger");
    work_from_trigger(jobs, trigger).expect("a readable trigger")
}

pub(crate) fn settings() -> QueueSettings {
    QueueSettings {
        start_id: id(7),
        outcome_wait: Duration::from_secs(5),
        retry_wait: Duration::from_millis(10),
        observer: Arc::new(IgnoreReports),
    }
}
