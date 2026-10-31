use br_agentic_contract::{Cause, ToolContent, ToolFault};
use br_runner_jobs::{Error, Refusal};

pub(super) enum Stop {
    Flag(String),
    Fault(Cause),
    Cancelled,
}

impl Stop {
    pub(super) fn fault(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Fault(Box::new(error))
    }

    pub(super) fn into_result(self, runner_type: &str) -> Result<ToolContent, ToolFault> {
        match self {
            Self::Flag(text) => flag(&text, "The call was refused."),
            Self::Fault(cause) => Err(ToolFault::new(cause)),
            Self::Cancelled => flag(
                &format!("The call to the runner {runner_type} was cancelled."),
                "The call was cancelled.",
            ),
        }
    }
}

fn flag(text: &str, fallback: &str) -> Result<ToolContent, ToolFault> {
    let text = if text.trim().is_empty() {
        fallback
    } else {
        text
    };
    ToolContent::error_text(text).map_err(ToolFault::new)
}

pub(super) fn refusal_text(error: &Error) -> String {
    match error {
        Error::Refused(refusal) => refusal_words(refusal),
        other => other.to_string(),
    }
}

fn refusal_words(refusal: &Refusal) -> String {
    if refusal.message.trim().is_empty() {
        refusal.reason.clone()
    } else {
        refusal.message.clone()
    }
}
