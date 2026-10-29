use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Duration;

use br_graph_contract::Cause;
use br_graph_executor::RunFailure;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailureKind {
    Permanent,
    Transient,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FailureReport {
    pub kind: FailureKind,
    pub reason_code: String,
    pub parameters: Map<String, Value>,
    pub diagnostic: String,
    pub retry_after: Option<Duration>,
}

impl FailureReport {
    pub fn permanent(reason_code: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            kind: FailureKind::Permanent,
            reason_code: reason_code.into(),
            parameters: Map::new(),
            diagnostic: text.into(),
            retry_after: None,
        }
    }
}

pub struct FailureContext<'a> {
    pub reason_code: &'a str,
    pub text: &'a str,
    pub cause: Option<&'a Cause>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FailureOverride {
    pub kind: Option<FailureKind>,
    pub reason_code: Option<String>,
    pub parameters: Option<Map<String, Value>>,
    pub diagnostic: Option<String>,
    pub retry_after: Option<Duration>,
}

pub type FailureMapping = Arc<dyn Fn(&FailureContext<'_>) -> FailureOverride + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub struct StartRefusal {
    pub reason_code: String,
    pub kind: Option<FailureKind>,
    pub parameters: Map<String, Value>,
    pub diagnostic: Option<String>,
    pub retry_after: Option<Duration>,
}

impl StartRefusal {
    pub fn new(reason_code: impl Into<String>) -> Self {
        Self {
            reason_code: reason_code.into(),
            kind: None,
            parameters: Map::new(),
            diagnostic: None,
            retry_after: None,
        }
    }

    pub fn into_report(self) -> FailureReport {
        FailureReport {
            kind: self.kind.unwrap_or(FailureKind::Permanent),
            reason_code: self.reason_code,
            parameters: self.parameters,
            diagnostic: self.diagnostic.unwrap_or_default(),
            retry_after: self.retry_after,
        }
    }
}

pub fn panic_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "the task panicked".to_owned())
}

pub(crate) fn executor_failure(failure: &RunFailure) -> (&'static str, String, Option<&Cause>) {
    match failure {
        RunFailure::NodeFailed { reason, cause, .. } => {
            ("node_failed", reason.clone(), cause.as_ref())
        }
        RunFailure::NodeRefusedInput { reason, .. } => {
            ("node_refused_an_input", reason.clone(), None)
        }
        RunFailure::NodeCouldNotStart { reason, cause, .. } => {
            ("node_could_not_start", reason.clone(), cause.as_ref())
        }
        RunFailure::NodeVanished { reason, .. } => ("node_vanished", reason.clone(), None),
        RunFailure::UpdateRefused { reason, .. } => ("update_refused", reason.clone(), None),
        RunFailure::EdgeFailed { reason, cause, .. } => {
            ("edge_failed", reason.clone(), cause.as_ref())
        }
        RunFailure::SinkRefused { reason, .. } => ("sink_refused", reason.clone(), None),
        RunFailure::CheckpointNotBuilt { reason } => ("checkpoint_not_built", reason.clone(), None),
        other => ("run_failed", other.to_string(), None),
    }
}

pub(crate) fn map_failure(
    code: &str,
    text: String,
    cause: Option<&Cause>,
    mapping: Option<&FailureMapping>,
) -> FailureReport {
    let base = FailureReport::permanent(code, text);
    let Some(mapping) = mapping else {
        return base;
    };
    let context = FailureContext {
        reason_code: &base.reason_code,
        text: &base.diagnostic,
        cause,
    };
    match catch_unwind(AssertUnwindSafe(|| mapping(&context))) {
        Ok(given) => FailureReport {
            kind: given.kind.unwrap_or(base.kind),
            reason_code: given.reason_code.unwrap_or(base.reason_code),
            parameters: given.parameters.unwrap_or(base.parameters),
            diagnostic: given.diagnostic.unwrap_or(base.diagnostic),
            retry_after: given.retry_after,
        },
        Err(payload) => FailureReport::permanent("panic", panic_text(payload.as_ref())),
    }
}
