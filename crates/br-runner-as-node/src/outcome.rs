use br_runner_jobs::{Content, Error, FailureReportView, ResultRef, SyncEnd};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refusal {
    pub reason: String,
    pub message: String,
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

    pub(crate) fn from_refusal(error: Error) -> Self {
        match error {
            Error::Refused(refusal) => Self::Refusal(Refusal {
                reason: refusal.reason,
                message: refusal.message,
            }),
            other => Self::Refusal(Refusal {
                reason: "refused".to_owned(),
                message: other.to_string(),
            }),
        }
    }
}
