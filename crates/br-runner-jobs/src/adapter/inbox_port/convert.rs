use br_agentic_contract::{InboxEntry, InboxItem, MailId, MailItem, PortError};
use br_llm_messages::{AgentId, Author, ConversationId, EntryId, IncomingEntry, RuntimeNotice};
use uuid::Uuid;

use super::notice_text;
use crate::book::{AgentBook, EntryKind, MailEntry, Recipients};
use crate::error::Error;
use crate::graphql;
use crate::model::{CallNotice, Mail, UnreadItem};

pub(in crate::adapter) struct Converted {
    pub item: InboxItem,
    pub entry: MailEntry,
}

fn cannot(what: &str) -> PortError {
    PortError::fault(Error::Unreadable(format!(
        "an unread item cannot be made into an item of the inbox port: {what}"
    )))
}

fn entry_id(uuid: Uuid) -> Result<EntryId, PortError> {
    EntryId::from_uuid(uuid).map_err(|_| cannot("an id is not a UUIDv7"))
}

fn agent_id(uuid: Uuid) -> Result<AgentId, PortError> {
    AgentId::from_uuid(uuid).map_err(|_| cannot("an agent id is not a UUIDv7"))
}

fn agent_ids(list: &[Uuid]) -> Result<Vec<AgentId>, PortError> {
    list.iter().map(|id| agent_id(*id)).collect()
}

fn mail_id(uuid: Uuid) -> Result<MailId, PortError> {
    MailId::from_uuid(uuid).map_err(|_| cannot("a mail id is not a UUIDv7"))
}

fn notice_entry(id: Uuid, text: String) -> Result<InboxItem, PortError> {
    let notice = RuntimeNotice::new(entry_id(id)?, text).map_err(|_| cannot("a blank notice"))?;
    InboxEntry::new(IncomingEntry::RuntimeNotice(notice))
        .map(InboxItem::Entry)
        .map_err(|_| cannot("a notice is not an entry of the inbox"))
}

async fn recipients(book: &AgentBook, agent: Uuid, mail: Uuid) -> Result<Recipients, PortError> {
    if let Some(kept) = book.recipients(mail) {
        return Ok(kept);
    }
    let core = book.core();
    let read = graphql::read_mail(core.backend.as_ref(), &core.ids(), mail, agent)
        .await
        .map_err(PortError::fault)?;
    let recipients = Recipients {
        to: read.to,
        cc: read.cc,
    };
    book.keep_recipients(mail, recipients.clone());
    Ok(recipients)
}

fn mail_to(mail: &Mail, agent: Uuid, recipients: &Recipients) -> Result<MailItem, PortError> {
    if !recipients.to.contains(&agent) {
        return Err(cannot("a direct mail does not list the agent in to"));
    }
    let source = mail
        .source_conversation
        .ok_or_else(|| cannot("a direct mail has no source conversation"))?;
    Ok(MailItem::To {
        id: mail_id(mail.mail_id)?,
        sender: agent_id(mail.from)?,
        author: Author::new(mail.author.clone()).map_err(|_| cannot("a blank author name"))?,
        source: ConversationId::from_uuid(source)
            .map_err(|_| cannot("a source conversation is not a UUIDv7"))?,
        to: agent_ids(&recipients.to)?,
        cc: agent_ids(&recipients.cc)?,
        subject: mail.subject.clone(),
        body: mail.body.clone(),
        attachment_names: mail
            .attachments
            .iter()
            .map(|attachment| attachment.name.clone())
            .collect(),
    })
}

fn call_entry(notice: &CallNotice) -> MailEntry {
    MailEntry {
        item_id: notice.item_id,
        child_job_id: notice.child_job_id,
        kind: EntryKind::CallNotice(notice.kind),
    }
}

pub(in crate::adapter) async fn convert(
    book: &AgentBook,
    agent: Uuid,
    unread: UnreadItem,
) -> Result<Converted, PortError> {
    match unread {
        UnreadItem::DirectMail { item_id, mail, .. } => {
            let recipients = recipients(book, agent, mail.mail_id).await?;
            Ok(Converted {
                item: InboxItem::Mail(mail_to(&mail, agent, &recipients)?),
                entry: MailEntry {
                    item_id,
                    child_job_id: None,
                    kind: EntryKind::DirectMail,
                },
            })
        }
        UnreadItem::CcNotice {
            item_id,
            mail_id: mail,
            subject,
            ..
        } => Ok(Converted {
            item: InboxItem::Mail(MailItem::Cc {
                id: mail_id(mail)?,
                subject,
            }),
            entry: MailEntry {
                item_id,
                child_job_id: None,
                kind: EntryKind::CcNotice,
            },
        }),
        UnreadItem::CallNotice(notice) => Ok(Converted {
            item: notice_entry(notice.item_id, notice_text::call_notice(&notice))?,
            entry: call_entry(&notice),
        }),
        UnreadItem::MailNotice { item_id, text, .. } => Ok(Converted {
            item: notice_entry(item_id, text)?,
            entry: MailEntry {
                item_id,
                child_job_id: None,
                kind: EntryKind::MailNotice,
            },
        }),
    }
}
