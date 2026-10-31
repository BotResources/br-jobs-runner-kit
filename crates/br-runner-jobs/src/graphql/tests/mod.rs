mod content;
mod documents;
mod lifting;
mod mail;
mod state;
mod unread;
mod unreadable;

use std::sync::Arc;

use serde_json::{Value, json};
use uuid::Uuid;

use super::WorkIds;
use crate::model::{NewMailAttachment, NewMailInput};
use crate::test_double::{Call, ScriptedJobs};

pub(super) const TEXT: &str = "The plain text of jobs, for an agent.";

pub(super) fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 + n)
}

pub(super) fn ids() -> WorkIds {
    WorkIds {
        job_id: id(1),
        run_id: id(2),
        instance_key: "instance-a".to_owned(),
    }
}

pub(super) fn work_json() -> Value {
    json!({ "jobId": id(1), "runId": id(2), "instanceKey": "instance-a" })
}

pub(super) fn jobs(answer: Value) -> Arc<ScriptedJobs> {
    let jobs = ScriptedJobs::new("runner", "instance-a");
    jobs.push_graphql(Ok(answer));
    jobs
}

pub(super) fn refusal(root: &str, reason: &str) -> Value {
    json!({ root: { "success": false, "reason": reason, "message": TEXT } })
}

pub(super) fn success(root: &str, fields: Value) -> Value {
    let mut body = json!({ "success": true, "reason": null, "message": null });
    if let (Some(target), Some(extra)) = (body.as_object_mut(), fields.as_object()) {
        target.extend(extra.clone());
    }
    json!({ root: body })
}

pub(super) fn sent(jobs: &ScriptedJobs) -> Vec<(String, Value)> {
    jobs.calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Graphql {
                document,
                variables,
            } => Some((document, variables)),
            _ => None,
        })
        .collect()
}

pub(super) fn only_call(jobs: &ScriptedJobs) -> (String, Value) {
    let mut calls = sent(jobs);
    assert_eq!(calls.len(), 1, "{calls:?}");
    calls.remove(0)
}

pub(super) fn content_value() -> Value {
    json!({
        "message": "hello",
        "attachments": [{ "name": "a.txt", "contentType": "text/plain", "content": "QQ==" }],
    })
}

pub(super) fn new_mail(attachments: Vec<NewMailAttachment>) -> NewMailInput {
    NewMailInput {
        mail_id: id(10),
        from_address: id(11),
        author: "An Author".to_owned(),
        source_conversation: id(12),
        subject: "subject".to_owned(),
        body: "body".to_owned(),
        to: vec![id(13), id(14)],
        cc: vec![id(15)],
        attachments,
    }
}

pub(super) fn new_attachment(n: u128, name: &str) -> NewMailAttachment {
    NewMailAttachment {
        attachment_id: id(20 + n),
        name: name.to_owned(),
        content_type: "text/plain".to_owned(),
        content: format!("content-{n}"),
    }
}
