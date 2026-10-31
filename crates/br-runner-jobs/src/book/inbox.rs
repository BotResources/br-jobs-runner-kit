use std::collections::{HashMap, HashSet};

use tokio::sync::mpsc;
use uuid::Uuid;

use crate::error::Error;
use crate::model::{CallNoticeKind, ItemArrived};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    DirectMail,
    CcNotice,
    CallNotice(CallNoticeKind),
    MailNotice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MailEntry {
    pub item_id: Uuid,
    pub child_job_id: Option<Uuid>,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Wake {
    Item(ItemArrived),
    FeedEnded(Error),
    BookDropped,
}

#[derive(Debug)]
pub(crate) struct ItemWatch {
    receiver: mpsc::UnboundedReceiver<Wake>,
}

impl ItemWatch {
    pub(crate) async fn next(&mut self) -> Wake {
        self.receiver.recv().await.unwrap_or(Wake::BookDropped)
    }
}

#[derive(Default)]
pub(super) struct Inbox {
    listed: HashMap<(Uuid, Uuid), MailEntry>,
    taken: HashSet<Uuid>,
    marked: HashMap<Uuid, HashSet<Uuid>>,
    watchers: Vec<(Uuid, mpsc::UnboundedSender<Wake>)>,
}

impl Inbox {
    pub(super) fn record_listed(&mut self, agent_id: Uuid, entry_id: Uuid, entry: MailEntry) {
        self.listed.insert((agent_id, entry_id), entry);
    }

    pub(super) fn listed(&self, agent_id: Uuid, entry_id: Uuid) -> Option<MailEntry> {
        self.listed.get(&(agent_id, entry_id)).copied()
    }

    pub(super) fn take_mail(&mut self, mail_id: Uuid) -> bool {
        self.taken.insert(mail_id)
    }

    pub(super) fn mail_taken(&self, mail_id: Uuid) -> bool {
        self.taken.contains(&mail_id)
    }

    pub(super) fn mark(&mut self, agent_id: Uuid, item_id: Uuid) {
        self.marked.entry(agent_id).or_default().insert(item_id);
    }

    pub(super) fn is_marked(&self, agent_id: Uuid, item_id: Uuid) -> bool {
        self.marked
            .get(&agent_id)
            .is_some_and(|items| items.contains(&item_id))
    }

    pub(super) fn marked_items(&self, agent_id: Uuid) -> HashSet<Uuid> {
        self.marked.get(&agent_id).cloned().unwrap_or_default()
    }

    pub(super) fn watch(&mut self, address: Uuid) -> ItemWatch {
        self.watchers.retain(|(_, sender)| !sender.is_closed());
        let (sender, receiver) = mpsc::unbounded_channel();
        self.watchers.push((address, sender));
        ItemWatch { receiver }
    }

    pub(super) fn wake_address(&mut self, item: &ItemArrived) {
        let Some(address) = item.address else {
            return;
        };
        self.watchers.retain(|(watched, sender)| {
            if *watched == address {
                sender.send(Wake::Item(*item)).is_ok()
            } else {
                !sender.is_closed()
            }
        });
    }

    pub(super) fn wake_all(&mut self, wake: &Wake) {
        self.watchers
            .retain(|(_, sender)| sender.send(wake.clone()).is_ok());
    }
}
