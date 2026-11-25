use serde_json::{Value, json};
use uuid::Uuid;

use super::call_notice;
use crate::model::{CallNotice, CallNoticeKind, FailureReportView};

fn failed_notice(diagnostic: Value) -> CallNotice {
    CallNotice {
        item_id: Uuid::nil(),
        address: None,
        kind: CallNoticeKind::JobFailed,
        notice_type: None,
        child_job_id: None,
        position: None,
        message: Some("Job x failed: model_call_failed.".to_owned()),
        report: Some(FailureReportView {
            kind: None,
            reason_code: "model_call_failed".to_owned(),
            params: json!({}),
            diagnostic,
        }),
        inline: None,
        result_ref: None,
    }
}

#[test]
fn given_a_failure_report_with_a_diagnostic_when_the_notice_is_written_then_its_cause_follows_the_reason_code() {
    let text = call_notice(&failed_notice(json!({ "text": "rejected by the provider (HTTP 400):\n bad" })));
    assert_eq!(
        text,
        "A call failed.\nJob x failed: model_call_failed.\nReason code: model_call_failed\n\
         Cause: model_call_failed: rejected by the provider (HTTP 400): bad"
    );
}

#[test]
fn given_a_failure_report_without_a_diagnostic_when_the_notice_is_written_then_it_names_the_reason_code_alone() {
    for diagnostic in [json!(null), json!({}), json!({ "text": "" })] {
        let text = call_notice(&failed_notice(diagnostic));
        assert!(text.ends_with("Reason code: model_call_failed"), "{text}");
        assert!(!text.contains("Cause:"));
    }
}
