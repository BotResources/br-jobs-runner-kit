use std::collections::HashSet;

use uuid::Uuid;

use super::{AgentBook, ItemWatch, MailEntry, Recipients};
use crate::model::UnreadItem;

impl AgentBook {
    pub(crate) fn record_listed(&self, agent_id: Uuid, entry_id: Uuid, entry: MailEntry) {
        self.inner.inbox().record_listed(agent_id, entry_id, entry);
    }

    pub(crate) fn listed(&self, agent_id: Uuid, entry_id: Uuid) -> Option<MailEntry> {
        self.inner.inbox().listed(agent_id, entry_id)
    }

    pub(crate) fn take_mail(&self, mail_id: Uuid) -> bool {
        self.inner.inbox().take_mail(mail_id)
    }

    #[cfg(test)]
    pub(crate) fn mail_taken(&self, mail_id: Uuid) -> bool {
        self.inner.inbox().mail_taken(mail_id)
    }

    pub(crate) fn mark(&self, agent_id: Uuid, item_id: Uuid) -> bool {
        self.inner.inbox().mark(agent_id, item_id)
    }

    pub(crate) fn keep_snapshot(&self, agent_id: Uuid, items: Vec<UnreadItem>) {
        self.inner.inbox().keep_snapshot(agent_id, items);
    }

    pub(crate) fn snapshot(&self, agent_id: Uuid) -> Option<Vec<UnreadItem>> {
        self.inner.inbox().snapshot(agent_id)
    }

    pub(crate) fn drop_snapshot(&self, agent_id: Uuid) {
        self.inner.inbox().drop_snapshot(agent_id);
    }

    pub(crate) fn keep_recipients(&self, mail_id: Uuid, recipients: Recipients) {
        self.inner.inbox().keep_recipients(mail_id, recipients);
    }

    pub(crate) fn recipients(&self, mail_id: Uuid) -> Option<Recipients> {
        self.inner.inbox().recipients(mail_id)
    }

    #[cfg(test)]
    pub(crate) fn is_marked(&self, agent_id: Uuid, item_id: Uuid) -> bool {
        self.inner.inbox().is_marked(agent_id, item_id)
    }

    pub(crate) fn marked_items(&self, agent_id: Uuid) -> HashSet<Uuid> {
        self.inner.inbox().marked_items(agent_id)
    }

    pub(crate) fn subscribe_items(&self, address: Uuid) -> ItemWatch {
        self.inner.inbox().watch(address)
    }
}
