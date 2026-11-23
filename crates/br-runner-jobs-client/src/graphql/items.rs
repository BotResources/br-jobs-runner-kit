use serde_json::Value;
use uuid::Uuid;

use super::decode;
use super::lift::unreadable;
use crate::error::Error;
use crate::model::{
    CallNotice, CallNoticeKind, FailureKind, FailureReportView, Mail, ResultRef, UnreadItem,
};

#[derive(Debug, Clone)]
pub(crate) struct BadItem {
    pub item_id: Option<Uuid>,
    pub child_job_id: Option<Uuid>,
    pub error: Error,
}

#[derive(Debug, Clone)]
pub(crate) enum Read {
    Item(UnreadItem),
    Bad(BadItem),
}

pub(super) fn read(entry: &Value) -> Read {
    match item(entry) {
        Ok(found) => Read::Item(found),
        Err(error) => Read::Bad(BadItem {
            item_id: decode::uuid(entry, "itemId").ok(),
            child_job_id: match entry.get("itemKind").and_then(Value::as_str) {
                Some("NOTICE") => decode::uuid(entry, "jobId").ok(),
                _ => None,
            },
            error,
        }),
    }
}

fn item(entry: &Value) -> Result<UnreadItem, Error> {
    let item_id = decode::uuid(entry, "itemId")?;
    let address = decode::opt_uuid(entry, "address")?;
    match decode::string(entry, "itemKind")?.as_str() {
        "MAIL" => Ok(UnreadItem::DirectMail {
            item_id,
            address,
            mail: direct_mail(entry)?,
        }),
        "CC_NOTICE" => Ok(UnreadItem::CcNotice {
            item_id,
            address,
            mail_id: decode::uuid(entry, "mailId")?,
            subject: decode::string(entry, "subject")?,
        }),
        "NOTICE" => Ok(UnreadItem::CallNotice(notice(entry, item_id, address)?)),
        "MAIL_NOTICE" => Ok(UnreadItem::MailNotice {
            item_id,
            address,
            mail_id: decode::opt_uuid(entry, "mailId")?,
            text: decode::string(entry, "message")?,
        }),
        other => Ok(UnreadItem::Unknown {
            item_id,
            address,
            item_kind: other.to_owned(),
            message: decode::opt_string(entry, "message")?,
        }),
    }
}

pub(super) fn author(entry: &Value) -> Result<String, Error> {
    let author = decode::string(entry, "author")?;
    if author.trim().is_empty() {
        return Err(unreadable("author"));
    }
    Ok(author)
}

fn direct_mail(entry: &Value) -> Result<Mail, Error> {
    Ok(Mail {
        mail_id: decode::uuid(entry, "mailId")?,
        from: decode::uuid(entry, "from")?,
        author: author(entry)?,
        source_conversation: decode::opt_uuid(entry, "sourceConversationId")?,
        subject: decode::string(entry, "subject")?,
        body: decode::string(entry, "body")?,
        to: Vec::new(),
        cc: Vec::new(),
        attachments: decode::mail_attachments(entry, "attachments")?,
    })
}

fn kind(notice_type: &str) -> CallNoticeKind {
    match notice_type {
        "ACCEPTED" => CallNoticeKind::Accepted,
        "QUEUE_POSITION" => CallNoticeKind::QueuePosition,
        "STARTED" => CallNoticeKind::RunStarted,
        "RUN_COMPLETED" => CallNoticeKind::RunCompleted,
        "JOB_FAILED" => CallNoticeKind::JobFailed,
        "JOB_CANCELLED" => CallNoticeKind::JobCancelled,
        "REFUSED" => CallNoticeKind::Refused,
        _ => CallNoticeKind::Unknown,
    }
}

fn notice(entry: &Value, item_id: Uuid, address: Option<Uuid>) -> Result<CallNotice, Error> {
    let notice_type = decode::string(entry, "noticeType")?;
    let kind = kind(&notice_type);
    let known = kind != CallNoticeKind::Unknown;
    Ok(CallNotice {
        item_id,
        address,
        kind,
        notice_type: (!known).then_some(notice_type),
        child_job_id: if known && kind != CallNoticeKind::Refused {
            Some(decode::uuid(entry, "jobId")?)
        } else {
            decode::opt_uuid(entry, "jobId")?
        },
        position: decode::opt_integer(entry, "position")?,
        message: decode::opt_string(entry, "message")?,
        report: optional(entry, "failureReport", report)?,
        inline: optional(entry, "answer", decode::answer)?,
        result_ref: decode::opt_uuid(entry, "resultId")?.map(ResultRef),
    })
}

fn optional<T>(
    entry: &Value,
    key: &str,
    read: fn(&Value) -> Result<T, Error>,
) -> Result<Option<T>, Error> {
    match entry.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(found) => read(found).map(Some),
    }
}

fn report(value: &Value) -> Result<FailureReportView, Error> {
    if !value.is_object() {
        return Err(unreadable("failureReport"));
    }
    Ok(FailureReportView {
        kind: match decode::opt_string(value, "kind")?.as_deref() {
            None => None,
            Some("TRANSIENT") => Some(FailureKind::Transient),
            Some("PERMANENT") => Some(FailureKind::Permanent),
            Some(_) => return Err(unreadable("failureReport")),
        },
        reason_code: decode::string(value, "reason_code")?,
        params: value.get("params").cloned().unwrap_or(Value::Null),
        diagnostic: value.get("diagnostic").cloned().unwrap_or(Value::Null),
    })
}
