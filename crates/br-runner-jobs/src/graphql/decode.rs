use serde_json::Value;
use uuid::Uuid;

use super::lift::unreadable;
use crate::error::Error;
use crate::model::{Content, ContentAttachment, MailAttachment};

fn present<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|found| !found.is_null())
}

fn uuid_of(value: &Value, what: &str) -> Result<Uuid, Error> {
    value
        .as_str()
        .and_then(|text| Uuid::parse_str(text).ok())
        .ok_or_else(|| unreadable(what))
}

pub(super) fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Value, Error> {
    present(value, key)
        .filter(|found| found.is_object())
        .ok_or_else(|| unreadable(key))
}

pub(super) fn uuid(value: &Value, key: &str) -> Result<Uuid, Error> {
    present(value, key)
        .ok_or_else(|| unreadable(key))
        .and_then(|found| uuid_of(found, key))
}

pub(super) fn opt_uuid(value: &Value, key: &str) -> Result<Option<Uuid>, Error> {
    present(value, key)
        .map(|found| uuid_of(found, key))
        .transpose()
}

pub(super) fn string(value: &Value, key: &str) -> Result<String, Error> {
    present(value, key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| unreadable(key))
}

pub(super) fn opt_string(value: &Value, key: &str) -> Result<Option<String>, Error> {
    present(value, key)
        .map(|found| {
            found
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| unreadable(key))
        })
        .transpose()
}

pub(super) fn opt_integer(value: &Value, key: &str) -> Result<Option<i64>, Error> {
    present(value, key)
        .map(|found| found.as_i64().ok_or_else(|| unreadable(key)))
        .transpose()
}

pub(super) fn list<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], Error> {
    present(value, key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| unreadable(key))
}

pub(super) fn uuids(value: &Value, key: &str) -> Result<Vec<Uuid>, Error> {
    list(value, key)?
        .iter()
        .map(|entry| uuid_of(entry, key))
        .collect()
}

fn attachment(entry: &Value, content_type: &str) -> Result<ContentAttachment, Error> {
    Ok(ContentAttachment {
        name: string(entry, "name")?,
        content_type: string(entry, content_type)?,
        content: string(entry, "content")?,
    })
}

pub(super) fn content_attachment(value: &Value) -> Result<ContentAttachment, Error> {
    attachment(value, "contentType")
}

pub(super) fn content(value: &Value) -> Result<Content, Error> {
    content_with(value, "contentType")
}

pub(super) fn answer(value: &Value) -> Result<Content, Error> {
    if !value.is_object() {
        return Err(unreadable("answer"));
    }
    content_with(value, "content_type")
}

fn content_with(value: &Value, content_type: &str) -> Result<Content, Error> {
    Ok(Content {
        message: string(value, "message")?,
        attachments: list(value, "attachments")?
            .iter()
            .map(|entry| attachment(entry, content_type))
            .collect::<Result<_, _>>()?,
    })
}

pub(super) fn mail_attachments(value: &Value, key: &str) -> Result<Vec<MailAttachment>, Error> {
    list(value, key)?
        .iter()
        .map(|entry| {
            Ok(MailAttachment {
                attachment_id: uuid(entry, "attachmentId")?,
                name: string(entry, "name")?,
                content_type: opt_string(entry, "contentType")?,
                size: opt_integer(entry, "size")?
                    .map(|size| u64::try_from(size).map_err(|_| unreadable("size")))
                    .transpose()?,
            })
        })
        .collect()
}
