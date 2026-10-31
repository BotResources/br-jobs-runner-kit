use serde_json::Value;

use super::decode;
use super::lift::unreadable;
use crate::error::Error;
use crate::model::{
    CallNotice, CallNoticeKind, FailureKind, FailureReportView, Mail, ResultRef, UnreadItem,
};

pub(super) fn item(entry: &Value) -> Result<UnreadItem, Error> {
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
        _ => Err(unreadable("an item of an unknown kind")),
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

fn kind(entry: &Value) -> Result<CallNoticeKind, Error> {
    match decode::string(entry, "noticeType")?.as_str() {
        "ACCEPTED" => Ok(CallNoticeKind::Accepted),
        "QUEUE_POSITION" => Ok(CallNoticeKind::QueuePosition),
        "STARTED" => Ok(CallNoticeKind::RunStarted),
        "RUN_COMPLETED" => Ok(CallNoticeKind::RunCompleted),
        "JOB_FAILED" => Ok(CallNoticeKind::JobFailed),
        _ => Err(unreadable("a notice of an unknown type")),
    }
}

fn notice(
    entry: &Value,
    item_id: uuid::Uuid,
    address: Option<uuid::Uuid>,
) -> Result<CallNotice, Error> {
    Ok(CallNotice {
        item_id,
        address,
        kind: kind(entry)?,
        child_job_id: Some(decode::uuid(entry, "jobId")?),
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
