use br_agentic_contract::{IncomingContext, NewMail, PortError};
use br_runner_jobs_client::{NewMailAttachment, NewMailInput, Write};
use uuid::Uuid;

use crate::adapter::incoming_port::resolve;
use crate::adapter::node_entry::Shared;

const DOCUMENT_CONTENT_TYPE: &str = "text/plain";

fn attachment_id(mail_id: Uuid, position: usize) -> Uuid {
    Uuid::from_u128(mail_id.as_u128() ^ (position as u128 + 1))
}

fn input_of(agent: Uuid, mail: &NewMail) -> NewMailInput {
    let mail_id = mail.id.as_uuid();
    let mut input = NewMailInput::new(
        mail_id,
        agent,
        mail.author.as_str(),
        mail.source.as_uuid(),
        mail.subject.clone(),
        mail.body.clone(),
    );
    input.to = mail.to.iter().map(|id| id.as_uuid()).collect();
    input.cc = mail.cc.iter().map(|id| id.as_uuid()).collect();
    input.attachments = mail
        .attachments
        .iter()
        .enumerate()
        .map(|(position, attachment)| NewMailAttachment {
            attachment_id: attachment_id(mail_id, position),
            name: attachment.name().to_owned(),
            content_type: DOCUMENT_CONTENT_TYPE.to_owned(),
            content: attachment.document().as_str().to_owned(),
        })
        .collect();
    input
}

pub(super) async fn send(
    shared: &Shared,
    context: &IncomingContext,
    mail: NewMail,
) -> Result<(), PortError> {
    if let Some(reason) = mail.refusal() {
        return Err(PortError::Refused(reason));
    }
    let resolved = resolve(shared, context).await?;
    drop(resolved.book.queue_writer().hand_over(Write::Mail {
        mail: input_of(resolved.agent, &mail),
    }));
    Ok(())
}
