use br_agentic_contract::{Tool, ToolBlock};
use br_llm_clients_core::Parameters;
use br_llm_messages::ToolName;
use br_runner_as_tool::{AuthorFailure, Payload, RunnerTool, ToolMode};
use br_runner_jobs::{CallerHandle, Content};

pub fn summary_tool(
    caller: CallerHandle,
    snapshot: String,
) -> Result<Tool, Box<dyn std::error::Error>> {
    let tool = RunnerTool::new(
        "summarizer",
        ToolName::new("summarize")?,
        "Summarizes the work of the agent so far.",
        Parameters::new(Vec::new())?,
        ToolMode::Sync,
        caller,
    )
    .payload_builder(move |arguments| {
        let snapshot = snapshot.clone();
        async move {
            if snapshot.trim().is_empty() {
                return Err(AuthorFailure::refused("There is nothing to summarize yet."));
            }
            let request = arguments.to_json_string().map_err(AuthorFailure::fault)?;
            Ok(Payload::Content(Content {
                message: format!("{snapshot}\n{request}"),
                attachments: Vec::new(),
            }))
        }
    })
    .result_processor(|result| async move {
        let block = ToolBlock::text(format!("Summary: {}", result.message.trim()))
            .map_err(AuthorFailure::fault)?;
        Ok(vec![block])
    })
    .build()?;
    Ok(tool)
}

fn main() {
    println!("summary_tool(caller_handle, snapshot_of_the_state) gives the Tool");
}
