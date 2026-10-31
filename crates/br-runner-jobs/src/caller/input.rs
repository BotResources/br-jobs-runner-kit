use serde_json::Value;
use uuid::Uuid;

use crate::model::{CallMode, PayloadForm};

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CreateChildInput {
    pub job_id: Uuid,
    pub command_id: Uuid,
    pub runner_type: String,
    pub mode: CallMode,
    pub config: Option<Value>,
    pub payload: PayloadForm,
    pub max_attempts: Option<i64>,
}

impl CreateChildInput {
    pub fn new(
        job_id: Uuid,
        command_id: Uuid,
        runner_type: impl Into<String>,
        mode: CallMode,
        payload: PayloadForm,
    ) -> Self {
        Self {
            job_id,
            command_id,
            runner_type: runner_type.into(),
            mode,
            config: None,
            payload,
            max_attempts: None,
        }
    }
}
