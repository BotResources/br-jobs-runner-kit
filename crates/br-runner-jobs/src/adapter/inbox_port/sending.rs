use br_agentic_contract::{InboxContext, NewMail, PortError};
use uuid::Uuid;

use super::resolve;
use crate::adapter::node_entry::Shared;
use crate::model::{NewMailAttachment, NewMailInput};
use crate::queue::Write;

const DOCUMENT_CONTENT_TYPE: &str = "text/plain";

fn attachment_id(mail_id: Uuid, position: usize) -> Uuid {
    Uuid::from_u128(mail_id.as_u128() ^ (position as u128 + 1))
}

fn input_of(agent: Uuid, mail: &NewMail) -> NewMailInput {
    let mail_id = mail.id.as_uuid();
    NewMailInput {
        mail_id,
        from_address: agent,
        author: mail.author.as_str().to_owned(),
        source_conversation: mail.source.as_uuid(),
        subject: mail.subject.clone(),
        body: mail.body.clone(),
        to: mail.to.iter().map(|id| id.as_uuid()).collect(),
        cc: mail.cc.iter().map(|id| id.as_uuid()).collect(),
        attachments: mail
            .attachments
            .iter()
            .enumerate()
            .map(|(position, attachment)| NewMailAttachment {
                attachment_id: attachment_id(mail_id, position),
                name: attachment.name().to_owned(),
                content_type: DOCUMENT_CONTENT_TYPE.to_owned(),
                content: attachment.document().as_str().to_owned(),
            })
            .collect(),
    }
}

pub(super) async fn send(
    shared: &Shared,
    context: &InboxContext,
    mail: NewMail,
) -> Result<(), PortError> {
    if let Some(reason) = mail.refusal() {
        return Err(PortError::Refused(reason));
    }
    let resolved = resolve(shared, context).await?;
    let (book, agent) = (&resolved.book, resolved.agent);
    if !book.take_mail(mail.id.as_uuid()) {
        return Ok(());
    }
    drop(book.queue_writer().hand_over(Write::Mail {
        mail: input_of(agent, &mail),
    }));
    Ok(())
}
