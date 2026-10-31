use crate::model::{CallNotice, CallNoticeKind, Content};

fn call_name(notice: &CallNotice) -> String {
    match notice.child_job_id {
        Some(job) => format!("The call {job}"),
        None => "A call".to_owned(),
    }
}

fn result_lines(inline: Option<&Content>, notice: &CallNotice) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(content) = inline {
        lines.push(format!("Result: {}", content.message));
        for attachment in &content.attachments {
            lines.push(format!("Result attachment: {}", attachment.name));
        }
    }
    if let Some(reference) = notice.result_ref {
        lines.push(format!(
            "The result is kept by jobs under the reference {}.",
            reference.as_uuid()
        ));
    }
    lines
}

fn failure_lines(notice: &CallNotice) -> Vec<String> {
    notice
        .report
        .iter()
        .map(|report| format!("Reason code: {}", report.reason_code))
        .collect()
}

pub(super) fn call_notice(notice: &CallNotice) -> String {
    let name = call_name(notice);
    let mut lines = vec![match notice.kind {
        CallNoticeKind::Accepted => format!("{name} was accepted."),
        CallNoticeKind::QueuePosition => match notice.position {
            Some(position) => format!("{name} waits at position {position} in the queue."),
            None => format!("{name} waits in the queue."),
        },
        CallNoticeKind::RunStarted => format!("{name} started."),
        CallNoticeKind::RunCompleted => format!("{name} completed."),
        CallNoticeKind::JobFailed => format!("{name} failed."),
        CallNoticeKind::JobCancelled => format!("{name} was cancelled."),
        CallNoticeKind::Refused => format!("{name} was refused."),
    }];
    if let Some(message) = &notice.message {
        lines.push(message.clone());
    }
    match notice.kind {
        CallNoticeKind::RunCompleted => lines.extend(result_lines(notice.inline.as_ref(), notice)),
        CallNoticeKind::JobFailed => lines.extend(failure_lines(notice)),
        _ => {}
    }
    lines.join("\n")
}
