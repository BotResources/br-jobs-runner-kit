mod inbox;
mod making;
mod no_ids;
mod outcomes;
mod waits;

use std::sync::Arc;
use std::time::Duration;

use br_core_auth::{AuthMethod, Passport, PassportClaims};
use contract_jobs::graphql_roots::*;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{AgentBook, RestoredAgent};
use crate::model::Content;
use crate::queue::{DeliveryQueue, IgnoreReports, QueueSettings};
use crate::test_double::{ScriptedJobs, trigger_json, work_from_trigger};
use crate::work::Work;

pub(crate) fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

pub(crate) const RUN: u128 = 2;
pub(crate) const JOB: u128 = 1;
pub(crate) const PARENT: u128 = 3;
pub(crate) const INITIAL: u128 = 4;
pub(crate) const USER: u128 = 5;
pub(crate) const IMPERSONATOR: u128 = 6;
pub(crate) const START: u128 = 7;

pub(crate) fn passport() -> Passport {
    Passport::human(
        id(USER),
        false,
        true,
        AuthMethod::Jwt,
        Some(id(IMPERSONATOR)),
        PassportClaims::new(),
    )
}

pub(crate) fn trigger() -> Value {
    trigger_json(id(RUN), id(JOB), id(PARENT), id(INITIAL), &passport()).expect("a trigger")
}

pub(crate) fn plain(root: &str) -> Value {
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
    let jobs = ScriptedJobs::new("runner", "instance-a");
    jobs.graphql_default(everything_succeeds());
    jobs
}

pub(crate) fn work(jobs: &Arc<ScriptedJobs>) -> Work {
    work_from_trigger(jobs, trigger()).expect("a readable trigger")
}

pub(crate) fn settings() -> QueueSettings {
    QueueSettings {
        start_id: id(START),
        outcome_wait: Duration::from_secs(5),
        retry_wait: Duration::from_millis(10),
        observer: Arc::new(IgnoreReports),
    }
}

pub(crate) struct Fixture {
    pub jobs: Arc<ScriptedJobs>,
    pub work: Work,
    pub queue: DeliveryQueue,
    pub book: AgentBook,
}

pub(crate) async fn fixture_on(
    jobs: Arc<ScriptedJobs>,
    restored: Vec<RestoredAgent>,
    can_restart: bool,
) -> Fixture {
    let work = work(&jobs);
    let (queue, _started) = work.delivery_queue(settings()).await.expect("a queue");
    let book = work
        .agent_book(&queue, restored, can_restart)
        .expect("a book");
    Fixture {
        jobs,
        work,
        queue,
        book,
    }
}

pub(crate) async fn fixture() -> Fixture {
    fixture_on(scripted(), Vec::new(), false).await
}

pub(crate) fn content(message: &str) -> Content {
    Content {
        message: message.to_owned(),
        attachments: Vec::new(),
    }
}
