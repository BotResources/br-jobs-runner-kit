use br_runner_jobs::{Content, FailureReportView, Refusal as JobsRefusal, ResultRef, SyncEnd};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refusal {
    pub reason: String,
    pub message: String,
}

impl Refusal {
    pub fn new(reason: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Outcome {
    Completed {
        inline: Option<Content>,
        result_ref: Option<ResultRef>,
    },
    Failed {
        report: Option<FailureReportView>,
        message: String,
    },
    Cancelled {
        message: String,
    },
    Refusal(Refusal),
}

impl Outcome {
    pub(crate) fn from_end(end: SyncEnd) -> Option<Self> {
        match end {
            SyncEnd::Completed { inline, result_ref } => {
                Some(Self::Completed { inline, result_ref })
            }
            SyncEnd::Failed { report, message } => Some(Self::Failed { report, message }),
            SyncEnd::Cancelled { message } => Some(Self::Cancelled { message }),
            _ => None,
        }
    }

    pub(crate) fn from_refusal(refusal: JobsRefusal) -> Self {
        Self::Refusal(Refusal::new(refusal.reason, refusal.message))
    }
}
