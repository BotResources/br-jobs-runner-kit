use br_agentic_contract::{Incoming, MailItem};
use br_llm_messages::{
    AgentId, AgentMail, Author, BlockId, ConversationId, IncomingEntry, PlainBlock, RuntimeNotice,
    TextBlock, Uuid,
};

use crate::error::{PortError, refusal};

fn recipients(agents: &[AgentId]) -> String {
    agents
        .iter()
        .map(|agent| agent.as_uuid().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn mail_text(
    subject: &str,
    to: &[AgentId],
    cc: &[AgentId],
    body: &str,
    attachment_names: &[String],
) -> String {
    format!(
        "Subject: {subject}\nTo: {}\nCc: {}\nAttachments: {}\n\n{body}",
        recipients(to),
        recipients(cc),
        attachment_names.join(", "),
    )
}

pub(crate) fn copy_notice_text(mail: &str, subject: &str) -> String {
    format!("Mail {mail} was sent in copy: {subject}")
}

pub(crate) struct MailOrigin {
    pub author: Author,
    pub source: ConversationId,
}

fn text_block(text: String) -> Result<PlainBlock, PortError> {
    let id = BlockId::from_uuid(Uuid::now_v7()).map_err(refusal)?;
    TextBlock::new(id, text)
        .map(PlainBlock::Text)
        .map_err(refusal)
}

pub(crate) fn incoming_entry(
    item: &Incoming,
    origin: impl Fn(AgentId) -> Result<MailOrigin, PortError>,
) -> Result<IncomingEntry, PortError> {
    match item {
        Incoming::Mail(MailItem::To {
            id,
            sender,
            to,
            cc,
            subject,
            body,
            attachment_names,
        }) => {
            let from = origin(*sender)?;
            let text = mail_text(subject, to, cc, body, attachment_names);
            AgentMail::new(
                id.entry_id(),
                *sender,
                from.author,
                from.source,
                vec![text_block(text)?],
            )
            .map(IncomingEntry::from)
            .map_err(refusal)
        }
        Incoming::Mail(MailItem::Cc { id, subject }) => {
            RuntimeNotice::new(id.entry_id(), copy_notice_text(&id.to_string(), subject))
                .map(IncomingEntry::from)
                .map_err(refusal)
        }
        Incoming::Entry(entry) => Ok(entry.entry().clone()),
        Incoming::Written(written) => {
            let from = origin(written.sender)?;
            AgentMail::new(
                written.id,
                written.sender,
                written.author.clone(),
                from.source,
                written.content.clone(),
            )
            .map(IncomingEntry::from)
            .map_err(refusal)
        }
    }
}
