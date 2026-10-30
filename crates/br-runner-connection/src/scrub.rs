use serde_json::{Map, Value};

use crate::credential::CredentialFailure;
use crate::error::{Error, Failure, Refusal};

const REDACTED: &str = "[redacted]";

fn text(value: String, secret: &str) -> String {
    if secret.is_empty() {
        value
    } else {
        value.replace(secret, REDACTED)
    }
}

fn json(value: Value, secret: &str) -> Value {
    match value {
        Value::String(s) => Value::String(text(s, secret)),
        Value::Array(items) => Value::Array(items.into_iter().map(|v| json(v, secret)).collect()),
        Value::Object(map) => Value::Object(object(map, secret)),
        other => other,
    }
}

fn object(map: Map<String, Value>, secret: &str) -> Map<String, Value> {
    map.into_iter()
        .map(|(key, value)| (text(key, secret), json(value, secret)))
        .collect()
}

fn failure(failure: Failure, secret: &str) -> Failure {
    Failure {
        cause: failure.cause,
        text: text(failure.text, secret),
    }
}

impl Error {
    pub(crate) fn scrubbed(self, secret: &str) -> Self {
        match self {
            Self::InvalidInput(reason) => Self::InvalidInput(text(reason, secret)),
            Self::Credential(f) => Self::Credential(CredentialFailure {
                kind: f.kind,
                text: text(f.text, secret),
            }),
            Self::Unavailable(f) => Self::Unavailable(failure(f, secret)),
            Self::Lost(f) => Self::Lost(failure(f, secret)),
            Self::Unauthenticated => Self::Unauthenticated,
            Self::Refused(refusal) => Self::Refused(Refusal {
                code: text(refusal.code, secret),
                params: object(refusal.params, secret),
                message: text(refusal.message, secret),
                origin: refusal.origin,
            }),
            Self::Unreadable(reason) => Self::Unreadable(text(reason, secret)),
        }
    }
}
