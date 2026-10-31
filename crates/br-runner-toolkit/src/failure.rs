use std::error::Error as StdError;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use br_runner_jobs::{Failed, FailureKind};
use serde_json::{Value, json};

pub struct ReportedFailure<'a> {
    pub reason_code: &'a str,
    pub text: &'a str,
    pub cause: Option<&'a (dyn StdError + Send + Sync + 'static)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FailureOverride {
    pub kind: Option<FailureKind>,
    pub reason_code: Option<String>,
    pub params: Option<Value>,
    pub diagnostic: Option<Value>,
    pub retry_after_seconds: Option<i64>,
}

pub trait FailureMapping: Send + Sync {
    fn map(&self, failure: &ReportedFailure<'_>) -> FailureOverride;
}

impl<F> FailureMapping for F
where
    F: Fn(&ReportedFailure<'_>) -> FailureOverride + Send + Sync,
{
    fn map(&self, failure: &ReportedFailure<'_>) -> FailureOverride {
        self(failure)
    }
}

pub fn failure_mapping<F>(mapping: F) -> Arc<dyn FailureMapping>
where
    F: Fn(&ReportedFailure<'_>) -> FailureOverride + Send + Sync + 'static,
{
    Arc::new(mapping)
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunRefusal {
    pub reason_code: String,
    pub kind: Option<FailureKind>,
    pub params: Option<Value>,
    pub diagnostic: Option<Value>,
    pub retry_after_seconds: Option<i64>,
}

impl RunRefusal {
    pub fn new(reason_code: impl Into<String>) -> Self {
        Self {
            reason_code: reason_code.into(),
            kind: None,
            params: None,
            diagnostic: None,
            retry_after_seconds: None,
        }
    }

    pub fn with_kind(mut self, kind: FailureKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn with_params(mut self, params: Value) -> Self {
        self.params = Some(params);
        self
    }

    pub fn with_diagnostic(mut self, diagnostic: Value) -> Self {
        self.diagnostic = Some(diagnostic);
        self
    }

    pub fn with_retry_after_seconds(mut self, seconds: i64) -> Self {
        self.retry_after_seconds = Some(seconds);
        self
    }

    pub(crate) fn into_failed(self) -> Failed {
        Failed {
            kind: self.kind.unwrap_or(FailureKind::Permanent),
            reason_code: self.reason_code,
            params: self.params.unwrap_or_else(|| json!({})),
            diagnostic: self.diagnostic.unwrap_or_else(|| json!("")),
            retry_after_seconds: self.retry_after_seconds,
        }
    }
}

pub(crate) struct Found {
    pub reason_code: String,
    pub text: String,
    pub cause: Option<Box<dyn StdError + Send + Sync + 'static>>,
    pub kind: FailureKind,
}

impl Found {
    pub(crate) fn permanent(reason_code: &str, text: impl Into<String>) -> Self {
        Self {
            reason_code: reason_code.to_owned(),
            text: text.into(),
            cause: None,
            kind: FailureKind::Permanent,
        }
    }

    pub(crate) fn transient(reason_code: &str, text: impl Into<String>) -> Self {
        Self {
            kind: FailureKind::Transient,
            ..Self::permanent(reason_code, text)
        }
    }
}

pub(crate) fn failed_of(found: Found, mapping: Option<&dyn FailureMapping>) -> Failed {
    let over = match mapping {
        None => FailureOverride::default(),
        Some(mapping) => {
            let reported = ReportedFailure {
                reason_code: &found.reason_code,
                text: &found.text,
                cause: found.cause.as_deref(),
            };
            match catch_unwind(AssertUnwindSafe(|| mapping.map(&reported))) {
                Ok(over) => over,
                Err(payload) => {
                    return Failed {
                        kind: found.kind,
                        reason_code: "panic".to_owned(),
                        params: json!({}),
                        diagnostic: Value::String(crate::panic::text_of(payload)),
                        retry_after_seconds: None,
                    };
                }
            }
        }
    };
    Failed {
        kind: over.kind.unwrap_or(found.kind),
        reason_code: over.reason_code.unwrap_or(found.reason_code),
        params: over.params.unwrap_or_else(|| json!({})),
        diagnostic: over.diagnostic.unwrap_or(Value::String(found.text)),
        retry_after_seconds: over.retry_after_seconds,
    }
}
