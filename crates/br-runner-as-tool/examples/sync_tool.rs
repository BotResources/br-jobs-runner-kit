use br_agentic_contract::Tool;
use br_llm_clients_core::{Parameter, ParameterKind, Parameters};
use br_llm_messages::ToolName;
use br_runner_as_tool::{RunnerTool, ToolMode};
use br_runner_jobs::CallerHandle;

pub fn analyst_tool(caller: CallerHandle) -> Result<Tool, Box<dyn std::error::Error>> {
    let question = Parameter::new(
        "question",
        "What the analyst must find out.",
        true,
        ParameterKind::String,
    )?;
    let tool = RunnerTool::new(
        "analyst",
        ToolName::new("ask_analyst")?,
        "Asks the analyst runner a question and waits for its answer.",
        Parameters::new(vec![question])?,
        ToolMode::Sync,
        caller,
    )
    .build()?;
    Ok(tool)
}

fn main() {
    println!("analyst_tool(caller_handle) gives the Tool to list in the tools port");
}
