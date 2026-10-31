use serde_json::{Value, json};
use uuid::Uuid;

use super::lift::call;
use super::{WorkIds, decode, documents, items};
use crate::backend::Backend;
use crate::error::{Error, Operation};
use crate::model::{Mail, UnreadItem};
use contract_jobs::graphql_roots::{MAIL_ROOT, UNREAD_ROOT};

pub(crate) async fn unread(
    backend: &dyn Backend,
    work: &WorkIds,
    address: Option<Uuid>,
) -> Result<Vec<UnreadItem>, Error> {
    let variables = json!({ "work": work.json(), "address": address });
    let root = call(
        backend,
        Operation::Read,
        UNREAD_ROOT,
        &documents::UNREAD,
        variables,
    )
    .await?;
    decode::list(&root, "items")?
        .iter()
        .map(items::item)
        .collect()
}

pub(crate) async fn read_mail(
    backend: &dyn Backend,
    work: &WorkIds,
    mail_id: Uuid,
    address: Uuid,
) -> Result<Mail, Error> {
    let variables = json!({ "work": work.json(), "mailId": mail_id, "address": address });
    let root = call(
        backend,
        Operation::Read,
        MAIL_ROOT,
        &documents::MAIL,
        variables,
    )
    .await?;
    mail(decode::object(&root, "mail")?)
}

fn mail(view: &Value) -> Result<Mail, Error> {
    Ok(Mail {
        mail_id: decode::uuid(view, "mailId")?,
        from: decode::uuid(view, "from")?,
        author: items::author(view)?,
        source_conversation: decode::opt_uuid(view, "sourceConversationId")?,
        subject: decode::string(view, "subject")?,
        body: decode::string(view, "body")?,
        to: decode::uuids(view, "to")?,
        cc: decode::uuids(view, "cc")?,
        attachments: decode::mail_attachments(view, "attachments")?,
    })
}
