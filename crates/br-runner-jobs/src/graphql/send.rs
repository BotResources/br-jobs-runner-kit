use serde_json::json;

use super::lift::call;
use super::{WorkIds, documents};
use crate::backend::Backend;
use crate::error::{Error, Operation};
use crate::model::NewMailInput;
use contract_jobs::graphql_roots::{SEND_MAIL_ROOT, UPLOAD_ATTACHMENT_ROOT};

pub(crate) async fn send_mail(
    backend: &dyn Backend,
    work: &WorkIds,
    mail: &NewMailInput,
) -> Result<(), Error> {
    for attachment in &mail.attachments {
        let variables = json!({ "input": {
            "work": work.json(),
            "mailId": mail.mail_id,
            "attachmentId": attachment.attachment_id,
            "fromAddress": mail.from_address,
            "name": attachment.name,
            "contentType": attachment.content_type,
            "content": attachment.content,
        } });
        call(
            backend,
            Operation::Mail,
            UPLOAD_ATTACHMENT_ROOT,
            &documents::UPLOAD_ATTACHMENT,
            variables,
        )
        .await?;
    }
    let variables = json!({ "input": {
        "work": work.json(),
        "mailId": mail.mail_id,
        "fromAddress": mail.from_address,
        "author": mail.author,
        "sourceConversationId": mail.source_conversation,
        "subject": mail.subject,
        "body": mail.body,
        "to": mail.to,
        "cc": mail.cc,
        "attachments": mail.attachments.iter().map(|attachment| json!({
            "attachmentId": attachment.attachment_id,
            "name": attachment.name,
        })).collect::<Vec<_>>(),
    } });
    call(
        backend,
        Operation::Mail,
        SEND_MAIL_ROOT,
        &documents::SEND_MAIL,
        variables,
    )
    .await
    .map(drop)
}
