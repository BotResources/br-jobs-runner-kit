mod background;
#[cfg(test)]
mod background_tests;
mod content;
mod hand;
mod prepare;
mod race;
mod stop;
mod sync_call;

use br_agentic_contract::{CancelView, PortFuture, ResultStream, ToolCode, ToolContent, ToolFault};
use br_llm_messages::OpaqueJson;
use br_runner_jobs::CallerHandle;
use serde_json::Value;

use crate::author::{PayloadBuilder, ResultProcessor};
use crate::mode::{PayloadKind, ToolMode};
use stop::Stop;

pub(crate) struct RunnerToolCode {
    pub(crate) runner_type: String,
    pub(crate) mode: ToolMode,
    pub(crate) caller: CallerHandle,
    pub(crate) config: Option<Value>,
    pub(crate) max_attempts: Option<i64>,
    pub(crate) kind: PayloadKind,
    pub(crate) keep: bool,
    pub(crate) builder: Option<PayloadBuilder>,
    pub(crate) processor: Option<ResultProcessor>,
}

impl ToolCode for RunnerToolCode {
    fn call<'a>(
        &'a self,
        arguments: &'a OpaqueJson,
        cancel: CancelView,
        _result: &'a mut dyn ResultStream,
    ) -> PortFuture<'a, Result<ToolContent, ToolFault>> {
        Box::pin(async move {
            match self.run(arguments, &cancel).await {
                Ok(content) => Ok(content),
                Err(stop) => stop.into_result(&self.runner_type),
            }
        })
    }
}

impl RunnerToolCode {
    async fn run(&self, arguments: &OpaqueJson, cancel: &CancelView) -> Result<ToolContent, Stop> {
        let prepared = self.prepare(arguments, cancel).await?;
        match self.mode {
            ToolMode::Sync => self.run_sync(prepared, cancel).await,
            ToolMode::Background { .. } => self.run_background(prepared, cancel),
        }
    }
}
