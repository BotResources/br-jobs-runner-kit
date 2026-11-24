use br_agentic_contract::{
    AgentLabel, PortError, PortFuture, PromptPort, RunContext, Tool, ToolsPort,
};

use crate::fields::Report;

pub struct StaticPrompt(pub String);

impl PromptPort<Report> for StaticPrompt {
    fn prompt<'a>(
        &'a self,
        _snapshot: &'a Report,
        _label: &'a AgentLabel,
    ) -> PortFuture<'a, Result<String, PortError>> {
        Box::pin(async move { Ok(self.0.clone()) })
    }
}

pub struct NoTools;

impl ToolsPort<Report> for NoTools {
    fn tools<'a>(
        &'a self,
        _snapshot: &'a Report,
        _context: &'a RunContext,
    ) -> PortFuture<'a, Result<Vec<Tool>, PortError>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
}
