use serde_json::{Map, Value};

use crate::error::{CodeOrigin, Error, Failure, Refusal};

const UNAUTHENTICATED: &str = "UNAUTHENTICATED";
const FORBIDDEN: &str = "FORBIDDEN";

pub(crate) struct Reply {
    pub(crate) status: u16,
    pub(crate) body: Vec<u8>,
}

fn optional<'a>(extensions: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    extensions.get(key).filter(|value| !value.is_null())
}

fn coded(error: &Value) -> Option<Refusal> {
    let extensions = error.get("extensions")?.as_object()?;
    let code = extensions.get("code")?.as_str()?.to_owned();
    let reason = match optional(extensions, "reason") {
        None => None,
        Some(value) => Some(value.as_str()?.to_owned()),
    };
    let params = match optional(extensions, "params") {
        None => Map::new(),
        Some(value) => value.as_object()?.clone(),
    };
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Some(Refusal {
        code,
        reason,
        params,
        message,
        origin: CodeOrigin::Service,
    })
}

fn unauthenticated(error: &Value) -> bool {
    coded(error).is_some_and(|refusal| refusal.code == UNAUTHENTICATED)
}

fn error_list(body: Option<&Value>) -> Result<Vec<&Value>, ()> {
    match body.and_then(|b| b.get("errors")) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items.iter().collect()),
        Some(_) => Err(()),
    }
}

fn data_of(body: &mut Value) -> Result<Value, Error> {
    match body.get_mut("data").map(Value::take) {
        Some(Value::Null) | None => Err(Error::Unreadable("the response holds no data".to_owned())),
        Some(data @ Value::Object(_)) => Ok(data),
        Some(_) => Err(Error::Unreadable("the data is not an object".to_owned())),
    }
}

pub(crate) fn judge(reply: Reply) -> Result<Value, Error> {
    if reply.status == 401 {
        return Err(Error::Unauthenticated);
    }
    let success = (200..300).contains(&reply.status);
    let mut body = serde_json::from_slice::<Value>(&reply.body).ok();
    let listed = error_list(body.as_ref());
    if let Ok(items) = &listed
        && items.iter().any(|e| unauthenticated(e))
    {
        return Err(Error::Unauthenticated);
    }
    let coded_errors: Vec<Refusal> = listed
        .as_ref()
        .map(|items| items.iter().filter_map(|e| coded(e)).collect())
        .unwrap_or_default();
    if let Some(first) = coded_errors.into_iter().next() {
        return Err(Error::Refused(first));
    }
    if !success {
        return Err(if reply.status == 403 {
            Error::Refused(Refusal {
                code: FORBIDDEN.to_owned(),
                reason: None,
                params: Map::new(),
                message: String::new(),
                origin: CodeOrigin::Connection,
            })
        } else {
            Error::Lost(Failure::transport(format!(
                "status {} without a readable code",
                reply.status
            )))
        });
    }
    let uncoded = match &listed {
        Err(()) => Some("the errors are not a list"),
        Ok(items) if !items.is_empty() => Some("an error without a code"),
        Ok(_) => None,
    };
    drop(listed);
    let Some(body) = body.as_mut() else {
        return Err(Error::Unreadable("the response is not JSON".to_owned()));
    };
    match uncoded {
        Some(reason) => Err(Error::Unreadable(reason.to_owned())),
        None => data_of(body),
    }
}
