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
    pub(crate) fn is_sealed(self) -> bool {
        matches!(self, Self::Completed | Self::BudgetExhausted)
    }

    pub(crate) fn reason(self, detail: Option<&str>) -> Result<ClosureReason, Error> {
        let bare = match self {
            Self::Completed => ClosureReason::completed(),
            Self::Cancelled => ClosureReason::cancelled(),
            Self::Interrupted => ClosureReason::interrupted(),
            Self::BudgetExhausted => ClosureReason::budget_exhausted(),
            Self::Failed => {
                let detail = detail.ok_or_else(|| Error::invalid("invalid_content"))?;
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
    pub thread: br_agentic_contract::ThreadRef,
    pub message_id: Uuid,
    pub kind: ClosureKind,
    pub detail: Option<String>,
    pub acting_user_id: Option<Uuid>,
    pub wait: Option<Duration>,
}
