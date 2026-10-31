use br_agentic_contract::{ToolBlock, ToolContent};
use br_runner_jobs::Content;

use super::Stop;

pub(super) fn default_content(runner_type: &str, result: &Content) -> Result<ToolContent, Stop> {
    let mut blocks = Vec::new();
    if !result.message.trim().is_empty() {
        blocks.push(text(&result.message)?);
    }
    for attachment in &result.attachments {
        blocks.push(text(&format!(
            "Attachment {} ({}):\n{}",
            attachment.name, attachment.content_type, attachment.content
        ))?);
    }
    if blocks.is_empty() {
        blocks.push(text(&format!(
            "The runner {runner_type} returned nothing."
        ))?);
    }
    Ok(ToolContent::new(blocks, false))
}

fn text(text: &str) -> Result<ToolBlock, Stop> {
    ToolBlock::text(text).map_err(Stop::fault)
}

pub(super) fn ended_text(runner_type: &str, outcome: &str, message: &str) -> String {
    format!("The runner {runner_type} {outcome}.\n{message}")
}
