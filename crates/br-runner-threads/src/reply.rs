use serde_json::{Map, Value};

use crate::error::{Error, Refusal};
use crate::transport::HttpReply;

const UNAUTHENTICATED: &str = "UNAUTHENTICATED";
const FORBIDDEN: &str = "FORBIDDEN";

struct Coded {
    code: String,
    params: Map<String, Value>,
}

fn coded_errors(body: &Value) -> Option<Vec<Option<Coded>>> {
    let errors = body.get("errors")?.as_array()?;
    Some(errors.iter().map(coded).collect())
}

fn coded(error: &Value) -> Option<Coded> {
    let extensions = error.get("extensions")?.as_object()?;
    let code = extensions.get("code")?.as_str()?.to_owned();
    let params = extensions
        .iter()
        .filter(|(key, _)| key.as_str() != "code")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Some(Coded { code, params })
}

fn verdict(errors: Vec<Option<Coded>>) -> Option<Error> {
    let first = errors.into_iter().flatten().collect::<Vec<_>>();
    if first.iter().any(|c| c.code == UNAUTHENTICATED) {
        return Some(Error::Unauthenticated);
    }
    first.into_iter().next().map(refused)
}

fn refused(coded: Coded) -> Error {
    Error::Refused(Refusal {
        code: coded.code,
        params: coded.params,
        command_id: None,
    })
}

pub(crate) fn data_of(reply: HttpReply) -> Result<Value, Error> {
    if reply.status == 401 {
        return Err(Error::Unauthenticated);
    }
    let body = serde_json::from_slice::<Value>(&reply.body).ok();
    let success = (200..300).contains(&reply.status);
    if !success {
        let readable = body.as_ref().and_then(coded_errors).and_then(verdict);
        return Err(match (readable, reply.status) {
            (Some(error), _) => error,
            (None, 403) => refused(Coded {
                code: FORBIDDEN.to_owned(),
                params: Map::new(),
            }),
            (None, status) => Error::AnswerLost(format!("status {status} without a readable code")),
        });
    }
    let body = body.ok_or_else(|| Error::Unreadable("the response is not JSON".to_owned()))?;
    if let Some(errors) = coded_errors(&body) {
        if errors.is_empty() {
            return data(body);
        }
        return Err(verdict(errors)
            .unwrap_or_else(|| Error::Unreadable("an error without a code".to_owned())));
    }
    data(body)
}

fn data(mut body: Value) -> Result<Value, Error> {
    match body.get_mut("data").map(Value::take) {
        Some(data @ Value::Object(_)) => Ok(data),
        _ => Err(Error::Unreadable("the response holds no data".to_owned())),
    }
}
