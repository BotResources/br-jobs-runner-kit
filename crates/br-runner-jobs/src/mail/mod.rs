#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::Arc;

use uuid::Uuid;

use crate::error::Error;
use crate::graphql;
use crate::model::{ContentAttachment, ListedAgent, Mail, NewMailInput, UnreadItem};
use crate::work::WorkCore;

#[derive(Clone)]
pub struct MailHandle {
    core: Arc<WorkCore>,
}

impl MailHandle {
    pub(crate) fn new(core: Arc<WorkCore>) -> Self {
        Self { core }
    }

    pub async fn send_mail(&self, mail: NewMailInput) -> Result<(), Error> {
        graphql::send_mail(self.core.backend.as_ref(), &self.core.ids(), &mail).await
    }

    pub async fn unread(&self, address: Uuid) -> Result<Vec<UnreadItem>, Error> {
        graphql::unread(self.core.backend.as_ref(), &self.core.ids(), Some(address)).await
    }

    pub async fn read_mail(&self, mail_id: Uuid, address: Uuid) -> Result<Mail, Error> {
        graphql::read_mail(
            self.core.backend.as_ref(),
            &self.core.ids(),
            mail_id,
            address,
        )
        .await
    }

    pub async fn attachment(
        &self,
        mail_id: Uuid,
        attachment_id: Uuid,
    ) -> Result<ContentAttachment, Error> {
        graphql::attachment(
            self.core.backend.as_ref(),
            &self.core.ids(),
            mail_id,
            attachment_id,
        )
        .await
    }

    pub async fn mark_read(&self, item_id: Uuid) -> Result<(), Error> {
        let backend = &self.core.backend;
        backend
            .mark_read(self.core.run_id, vec![item_id], backend.reply_wait())
            .await
    }

    pub async fn list_agents(&self) -> Result<Vec<ListedAgent>, Error> {
        let backend = &self.core.backend;
        backend
            .list_agents(self.core.run_id, backend.reply_wait())
            .await
    }
}

impl fmt::Debug for MailHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MailHandle").finish_non_exhaustive()
    }
}

impl fmt::Display for MailHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a mail handle")
    }
}
