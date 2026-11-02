use br_agentic_contract::{InboxContext, MailDocument, MailId, PortError};
use futures::future::try_join_all;

use super::resolve;
use crate::adapter::node_entry::Shared;
use crate::book::Recipients;
use crate::error::Error;
use crate::graphql;
use crate::reason::RefusalReason;

fn from_jobs(error: Error) -> PortError {
    match &error {
        Error::Refused(refusal) if refusal.reason == RefusalReason::MailNotYours => {
            PortError::refused(refusal.message.clone())
        }
        _ => PortError::fault(error),
    }
}

pub(super) async fn documents(
    shared: &Shared,
    context: &InboxContext,
    mail: MailId,
) -> Result<Vec<MailDocument>, PortError> {
    let resolved = resolve(shared, context).await?;
    let (book, agent) = (&resolved.book, resolved.agent);
    let core = book.core();
    let ids = core.ids();
    let backend = core.backend.as_ref();
    let mail_id = mail.as_uuid();
    let read = graphql::read_mail(backend, &ids, mail_id, agent)
        .await
        .map_err(from_jobs)?;
    if !read.to.contains(&agent) {
        return Err(PortError::refused(
            "the agent is not a recipient in to of this mail",
        ));
    }
    let fetched =
        try_join_all(read.attachments.iter().map(|attachment| {
            graphql::attachment(backend, &ids, mail_id, attachment.attachment_id)
        }))
        .await
        .map_err(from_jobs)?;
    book.keep_recipients(
        mail_id,
        Recipients {
            to: read.to,
            cc: read.cc,
        },
    );
    Ok(fetched
        .into_iter()
        .map(|attachment| MailDocument {
            name: attachment.name,
            content: attachment.content.into_bytes(),
        })
        .collect())
}
