use std::future::Future;
use std::sync::Arc;

use br_agentic_contract::{Tool, ToolBlock};
use br_llm_clients_core::{Parameters, ToolSpec};
use br_llm_messages::{OpaqueJson, ToolName};
use br_runner_jobs::{CallerHandle, Content, Error};
use contract_jobs::segment::{SegmentError, SubjectSegment};
use serde_json::Value;

use crate::author::{self, AuthorFailure, Payload, PayloadBuilder, ResultProcessor};
use crate::call::RunnerToolCode;
use crate::error::RunnerToolError;
use crate::mode::{PayloadKind, ToolMode};

#[must_use = "a RunnerTool gives nothing until it is built"]
pub struct RunnerTool {
    runner_type: String,
    name: ToolName,
    description: String,
    parameters: Parameters,
    mode: ToolMode,
    caller: CallerHandle,
    config: Option<Value>,
    max_attempts: Option<i64>,
    kind: PayloadKind,
    keep: bool,
    builder: Option<PayloadBuilder>,
    processor: Option<ResultProcessor>,
}

impl RunnerTool {
    pub fn new(
        runner_type: impl Into<String>,
        name: ToolName,
        description: impl Into<String>,
        parameters: Parameters,
        mode: ToolMode,
        caller: CallerHandle,
    ) -> Self {
        Self {
            runner_type: runner_type.into(),
            name,
            description: description.into(),
            parameters,
            mode,
            caller,
            config: None,
            max_attempts: None,
            kind: PayloadKind::Inline,
            keep: false,
            builder: None,
            processor: None,
        }
    }

    pub fn config(mut self, config: Value) -> Self {
        self.config = Some(config);
        self
    }

    pub fn max_attempts(mut self, max_attempts: i64) -> Self {
        self.max_attempts = Some(max_attempts);
        self
    }

    pub fn payload_kind(mut self, kind: PayloadKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn keep_result(mut self, keep: bool) -> Self {
        self.keep = keep;
        self
    }

    pub fn payload_builder<F, Fut>(mut self, function: F) -> Self
    where
        F: Fn(OpaqueJson) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Payload, AuthorFailure>> + Send + 'static,
    {
        self.builder = Some(author::payload_builder(function));
        self
    }

    pub fn result_processor<F, Fut>(mut self, function: F) -> Self
    where
        F: Fn(Content) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<ToolBlock>, AuthorFailure>> + Send + 'static,
    {
        self.processor = Some(author::result_processor(function));
        self
    }

    pub fn build(self) -> Result<Tool, RunnerToolError> {
        SubjectSegment::runner_type(&self.runner_type)
            .map_err(|error| RunnerToolError::RunnerType(segment_error(&error)))?;
        let spec = ToolSpec::new(self.name, self.description, self.parameters)
            .map_err(RunnerToolError::Spec)?;
        let code = RunnerToolCode {
            runner_type: self.runner_type,
            mode: self.mode,
            caller: self.caller,
            config: self.config,
            max_attempts: self.max_attempts,
            kind: self.kind,
            keep: self.keep,
            builder: self.builder,
            processor: self.processor,
        };
        Ok(Tool::new(spec, Arc::new(code)))
    }
}

fn segment_error(error: &SegmentError) -> Error {
    let field = match error {
        SegmentError::Blank { field }
        | SegmentError::TooLong { field, .. }
        | SegmentError::Invalid { field, .. } => field,
    };
    Error::InvalidInput(format!(
        "the {field} is not a valid segment: {}",
        error.code()
    ))
}
