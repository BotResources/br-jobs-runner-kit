use serde_json::Value;

use crate::backend::Backend;
use crate::error::{Error, Operation};

pub(super) fn unreadable(what: &str) -> Error {
    Error::Unreadable(format!("the reply of jobs cannot be read: {what}"))
}

fn retag(error: Error, operation: Operation) -> Error {
    match error {
        Error::Refused(mut refusal) => {
            refusal.operation = operation;
            Error::Refused(refusal)
        }
        other => other,
    }
}

fn text(root: &Value, key: &str) -> Result<String, Error> {
    root.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| unreadable("a refusal without its reason or its text"))
}

pub(super) async fn call(
    backend: &dyn Backend,
    operation: Operation,
    root_name: &str,
    document: &str,
    variables: Value,
) -> Result<Value, Error> {
    let mut data = backend
        .graphql(document, variables)
        .await
        .map_err(|error| retag(error, operation))?;
    let root = data
        .get_mut(root_name)
        .map(Value::take)
        .filter(Value::is_object)
        .ok_or_else(|| unreadable(&format!("no {root_name} in the reply")))?;
    match root.get("success").and_then(Value::as_bool) {
        Some(true) => Ok(root),
        Some(false) => Err(Error::refused(
            operation,
            text(&root, "reason")?,
            refusal_message(&root)?,
        )),
        None => Err(unreadable("no success flag in the reply")),
    }
}

fn refusal_message(root: &Value) -> Result<String, Error> {
    let mut message = text(root, "message")?;
    let bad = root
        .get("badAddresses")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for entry in bad {
        let line = bad_address_line(entry).ok_or_else(|| unreadable("a bad address"))?;
        message.push('\n');
        message.push_str(&line);
    }
    Ok(message)
}

fn bad_address_line(entry: &Value) -> Option<String> {
    let field = |key: &str| entry.get(key).and_then(Value::as_str);
    Some(format!(
        "{}: {} ({})",
        field("address")?,
        field("message")?,
        field("reason")?
    ))
}
