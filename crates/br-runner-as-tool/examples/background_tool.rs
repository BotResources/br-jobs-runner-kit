use br_agentic_contract::Tool;
use br_llm_clients_core::{Parameter, ParameterKind, Parameters};
use br_llm_messages::ToolName;
use br_runner_as_tool::{PayloadKind, RunnerTool, ToolMode};
use br_runner_jobs::CallerHandle;
use uuid::Uuid;

pub fn report_writer_tool(
    caller: CallerHandle,
    calling_agent: Uuid,
) -> Result<Tool, Box<dyn std::error::Error>> {
    let topic = Parameter::new(
        "topic",
        "What the report is about.",
        true,
        ParameterKind::String,
    )?;
    let tool = RunnerTool::new(
        "report-writer",
        ToolName::new("write_report")?,
        "Starts a long report. The answer comes later by mail.",
        Parameters::new(vec![topic])?,
        ToolMode::Background {
            caller_address: calling_agent,
        },
        caller,
    )
    .payload_kind(PayloadKind::Kept)
    .build()?;
    Ok(tool)
}

fn main() {
    println!("report_writer_tool(caller_handle, address_of_the_calling_agent) gives the Tool");
}
