use std::time::Duration;

use br_llm_messages::ClosureReason;
use uuid::Uuid;

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClosureKind {
    Completed,
    Cancelled,
    Interrupted,
    BudgetExhausted,
    Failed,
}

impl ClosureKind {
    pub(crate) fn of(reason: &ClosureReason) -> Result<Self, Error> {
        Ok(match reason {
            ClosureReason::Completed { .. } => Self::Completed,
            ClosureReason::Cancelled { .. } => Self::Cancelled,
            ClosureReason::Interrupted { .. } => Self::Interrupted,
            ClosureReason::BudgetExhausted { .. } => Self::BudgetExhausted,
            ClosureReason::Failed { .. } => Self::Failed,
            _ => {
                return Err(Error::invalid(
                    "a closure reason this library does not know",
                ));
            }
        })
    }

    pub(crate) fn reason(self, detail: Option<&str>) -> Result<ClosureReason, Error> {
        let bare = match self {
            Self::Completed => ClosureReason::completed(),
            Self::Cancelled => ClosureReason::cancelled(),
            Self::Interrupted => ClosureReason::interrupted(),
            Self::BudgetExhausted => ClosureReason::budget_exhausted(),
            Self::Failed => {
                let detail =
                    detail.ok_or_else(|| Error::invalid("a Failed closing needs a detail"))?;
                return ClosureReason::failed(detail).map_err(|e| Error::library(&e));
            }
        };
        match detail {
            Some(detail) => bare.with_detail(detail).map_err(|e| Error::library(&e)),
            None => Ok(bare),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Closing {
    pub command_id: Uuid,
    pub thread_id: Uuid,
    pub message_id: Uuid,
    pub kind: ClosureKind,
    pub detail: Option<String>,
    pub acting_user_id: Option<Uuid>,
    pub wait: Duration,
}
