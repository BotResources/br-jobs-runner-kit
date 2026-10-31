use uuid::Uuid;

use super::Arrival;
use crate::error::Error;
use crate::graphql::{BadItem, Read};
use crate::model::{CallNotice, CallNoticeKind, ItemArrived, ItemKind, SyncEnd, UnreadItem};

pub(super) fn item_arrived(item: &UnreadItem) -> ItemArrived {
    ItemArrived {
        address: item.address(),
        item_id: item.item_id(),
        kind: match item {
            UnreadItem::DirectMail { .. } => ItemKind::DirectMail,
            UnreadItem::CcNotice { .. } => ItemKind::CcNotice,
            UnreadItem::CallNotice(_) => ItemKind::CallNotice,
            UnreadItem::MailNotice { .. } => ItemKind::MailNotice,
            UnreadItem::Unknown { .. } => ItemKind::Unknown,
        },
    }
}

pub(super) fn bad(bad: &BadItem) -> Arrival {
    Arrival::Unreadable {
        child_job_id: bad.child_job_id,
        error: bad.error.clone(),
    }
}

pub(super) fn resolved(read: &Read, event: &ItemArrived) -> Option<Arrival> {
    match read {
        Read::Item(item) if item.item_id() == event.item_id => sync_end(item),
        Read::Bad(failed) if failed.item_id.is_none_or(|id| id == event.item_id) => {
            Some(bad(failed))
        }
        _ => None,
    }
}

pub(super) fn is_run_level_notice(event: &ItemArrived) -> bool {
    event.address.is_none() && event.kind == ItemKind::CallNotice
}

fn unreadable(child_job_id: Option<Uuid>, what: &str) -> Arrival {
    Arrival::Unreadable {
        child_job_id,
        error: Error::Unreadable(format!("the end of a sync call cannot be read: {what}")),
    }
}

fn ended(notice: &CallNotice, child_job_id: Uuid) -> Arrival {
    let message = notice.message.clone();
    match notice.kind {
        CallNoticeKind::RunCompleted => {
            if notice.inline.is_none() && notice.result_ref.is_none() {
                return unreadable(Some(child_job_id), "a completion with no Result");
            }
            Arrival::SyncEnd {
                child_job_id,
                end: SyncEnd::Completed {
                    inline: notice.inline.clone(),
                    result_ref: notice.result_ref,
                },
            }
        }
        CallNoticeKind::JobFailed => match message {
            Some(message) => Arrival::SyncEnd {
                child_job_id,
                end: SyncEnd::Failed {
                    report: notice.report.clone(),
                    message,
                },
            },
            None => unreadable(Some(child_job_id), "a failure with no message"),
        },
        _ => match message {
            Some(message) => Arrival::SyncEnd {
                child_job_id,
                end: SyncEnd::Cancelled { message },
            },
            None => unreadable(Some(child_job_id), "a cancel with no message"),
        },
    }
}

pub(super) fn sync_end(item: &UnreadItem) -> Option<Arrival> {
    let UnreadItem::CallNotice(notice) = item else {
        return None;
    };
    if notice.address.is_some() {
        return None;
    }
    if !matches!(
        notice.kind,
        CallNoticeKind::RunCompleted | CallNoticeKind::JobFailed | CallNoticeKind::JobCancelled
    ) {
        return None;
    }
    Some(match notice.child_job_id {
        Some(child_job_id) => ended(notice, child_job_id),
        None => unreadable(None, "a run-level notice with no child Job"),
    })
}
