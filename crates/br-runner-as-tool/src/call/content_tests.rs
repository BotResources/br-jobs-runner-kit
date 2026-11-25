use serde_json::json;

use super::{CAUSE_CHARS, cause_line, ended_text};

#[test]
fn given_a_diagnostic_text_when_the_cause_is_read_then_it_names_the_reason_code_and_the_text() {
    let cause = cause_line(
        "model_call_failed",
        &json!({ "text": "the client failed: rejected by the provider (HTTP 400): bad model" }),
    );
    assert_eq!(
        cause.as_deref(),
        Some(
            "Cause: model_call_failed: the client failed: rejected by the provider (HTTP 400): bad model"
        )
    );
}

#[test]
fn given_no_diagnostic_when_the_cause_is_read_then_there_is_none() {
    for diagnostic in [
        json!(null),
        json!({}),
        json!({ "text": "  " }),
        json!("bare"),
    ] {
        assert_eq!(cause_line("failed", &diagnostic), None, "{diagnostic}");
    }
}

#[test]
fn given_a_diagnostic_object_without_text_when_the_cause_is_read_then_it_is_the_object_as_json() {
    let cause = cause_line("failed", &json!({ "status": 400 }));
    assert_eq!(cause.as_deref(), Some(r#"Cause: failed: {"status":400}"#));
}

#[test]
fn given_a_long_diagnostic_on_several_lines_when_the_cause_is_read_then_it_is_one_line_cut() {
    let long = format!("first\n  second {}", "x".repeat(2 * CAUSE_CHARS));
    let cause = cause_line("failed", &json!({ "text": long })).unwrap();
    let text = cause.strip_prefix("Cause: failed: ").unwrap();
    assert!(!text.contains('\n'));
    assert!(text.starts_with("first second x"));
    assert_eq!(text.chars().count(), CAUSE_CHARS + 1);
    assert!(text.ends_with('…'));
}

#[test]
fn given_an_outcome_when_the_ended_text_is_made_then_the_message_of_jobs_follows_unchanged() {
    assert_eq!(
        ended_text("analyst", "failed", "Job x failed: model_call_failed."),
        "The runner analyst failed.\nJob x failed: model_call_failed."
    );
}
