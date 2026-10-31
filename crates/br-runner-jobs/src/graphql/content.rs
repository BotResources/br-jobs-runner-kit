use serde_json::{Value, json};
use uuid::Uuid;

use super::lift::call;
use super::{WorkIds, decode, documents};
use crate::backend::Backend;
use crate::error::{Error, Operation};
use crate::model::{Content, ContentAttachment, ResultRef};
use contract_jobs::graphql_roots::{
    ATTACHMENT_ROOT, PAYLOAD_ROOT, READ_RESULT_ROOT, UPLOAD_PAYLOAD_ROOT, UPLOAD_RESULT_ROOT,
};

fn content_json(content: &Content) -> Value {
    json!({
        "message": content.message,
        "attachments": content.attachments.iter().map(|attachment| json!({
            "name": attachment.name,
            "contentType": attachment.content_type,
            "content": attachment.content,
        })).collect::<Vec<_>>(),
    })
}

pub(crate) async fn payload(backend: &dyn Backend, work: &WorkIds) -> Result<Content, Error> {
    let variables = json!({ "work": work.json() });
    let root = call(
        backend,
        Operation::Payload,
        PAYLOAD_ROOT,
        &documents::PAYLOAD,
        variables,
    )
    .await?;
    decode::content(decode::object(&root, "content")?)
}

pub(crate) async fn upload_payload(
    backend: &dyn Backend,
    work: &WorkIds,
    for_job_id: Uuid,
    content: &Content,
) -> Result<(), Error> {
    let variables = json!({ "input": {
        "forJobId": for_job_id,
        "content": content_json(content),
        "work": work.json(),
    } });
    call(
        backend,
        Operation::Upload,
        UPLOAD_PAYLOAD_ROOT,
        &documents::UPLOAD_PAYLOAD,
        variables,
    )
    .await
    .map(drop)
}

pub(crate) async fn upload_result(
    backend: &dyn Backend,
    work: &WorkIds,
    answer: &Content,
) -> Result<(), Error> {
    let variables = json!({ "input": { "work": work.json(), "answer": content_json(answer) } });
    call(
        backend,
        Operation::Result,
        UPLOAD_RESULT_ROOT,
        &documents::UPLOAD_RESULT,
        variables,
    )
    .await
    .map(drop)
}

pub(crate) async fn read_result(
    backend: &dyn Backend,
    work: &WorkIds,
    result: ResultRef,
    keep: bool,
) -> Result<Content, Error> {
    let variables = json!({ "input": {
        "work": work.json(),
        "resultId": result.as_uuid(),
        "keep": keep,
    } });
    let root = call(
        backend,
        Operation::Result,
        READ_RESULT_ROOT,
        &documents::READ_RESULT,
        variables,
    )
    .await?;
    decode::content(decode::object(&root, "content")?)
}

pub(crate) async fn attachment(
    backend: &dyn Backend,
    work: &WorkIds,
    mail_id: Uuid,
    attachment_id: Uuid,
) -> Result<ContentAttachment, Error> {
    let variables = json!({
        "work": work.json(),
        "mailId": mail_id,
        "attachmentId": attachment_id,
    });
    let root = call(
        backend,
        Operation::Attachment,
        ATTACHMENT_ROOT,
        &documents::ATTACHMENT,
        variables,
    )
    .await?;
    decode::content_attachment(decode::object(&root, "attachment")?)
}
