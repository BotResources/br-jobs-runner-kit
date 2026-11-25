use br_agentic_contract::{ToolBlock, ToolContent};
use br_runner_jobs_client::{Content, FailureReportView};

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

/// The longest diagnostic text a failure's cause line carries.
pub(super) const CAUSE_CHARS: usize = 500;

/// The text of a failed child: the outcome, the message of jobs and, when
/// the child's failure report holds a diagnostic, a last line
/// `Cause: <reason code>: <diagnostic text>` — the child's own statement of
/// the technical cause, which the message of jobs does not hold.
pub(super) fn failed_text(
    runner_type: &str,
    message: &str,
    report: Option<&FailureReportView>,
) -> String {
    let text = ended_text(runner_type, "failed", message);
    match report.and_then(|report| cause_line(&report.reason_code, &report.diagnostic)) {
        Some(cause) => format!("{text}\n{cause}"),
        None => text,
    }
}

/// `Cause: <reason code>: <diagnostic text>`, when the report holds a
/// diagnostic: its `text` member when it is a non-blank string, else the
/// diagnostic itself as compact JSON when it is a non-empty object. On one
/// line, cut at [`CAUSE_CHARS`] characters.
fn cause_line(reason_code: &str, diagnostic: &serde_json::Value) -> Option<String> {
    let diagnostic = diagnostic_text(diagnostic)?;
    Some(format!("Cause: {reason_code}: {}", one_line(&diagnostic)))
}

fn diagnostic_text(diagnostic: &serde_json::Value) -> Option<String> {
    if let Some(text) = diagnostic.get("text").and_then(serde_json::Value::as_str) {
        return (!text.trim().is_empty()).then(|| text.to_owned());
    }
    match diagnostic.as_object() {
        Some(object) if !object.is_empty() => Some(diagnostic.to_string()),
        _ => None,
    }
}

fn one_line(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= CAUSE_CHARS {
        return line;
    }
    let mut cut: String = line.chars().take(CAUSE_CHARS).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;
