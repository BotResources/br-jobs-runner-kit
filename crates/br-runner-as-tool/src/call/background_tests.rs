use br_graph_contract::CancelToken;
use br_runner_jobs::test_double::{Answer, MemoryCall, MemoryScript, memory_caller};
use br_runner_jobs::{PayloadForm, Removal, WriteResult};
use uuid::Uuid;

use super::{RunnerToolCode, Stop};
use crate::mode::{PayloadKind, ToolMode};

fn code(script_answer: Answer<WriteResult>) -> (RunnerToolCode, MemoryScript) {
    let (caller, script) = memory_caller();
    script.create(script_answer);
    let code = RunnerToolCode {
        runner_type: "analyst".to_owned(),
        mode: ToolMode::Background {
            caller_address: Uuid::from_u128(1),
        },
        caller,
        config: None,
        max_attempts: None,
        kind: PayloadKind::Inline,
        keep: false,
        builder: None,
        processor: None,
    };
    (code, script)
}

fn created_then_triggered(answer: Answer<WriteResult>) -> (Result<(), Stop>, usize) {
    let (code, script) = code(answer);
    let child = Uuid::from_u128(2);
    let form = PayloadForm::Kept;
    let created = code
        .hand_create(child, form, &CancelToken::new().view())
        .ok()
        .expect("a create");
    let token = CancelToken::new();
    token.handle().trigger();
    let outcome = code.stop_if_cancelled(child, created, &token.view());
    let cancels = script
        .calls()
        .iter()
        .filter(|call| matches!(call, MemoryCall::Cancel { .. }))
        .count();
    (outcome, cancels)
}

#[test]
fn given_a_create_taken_when_the_view_triggers_then_the_call_stops_with_one_cancel() {
    for answer in [Answer::delivered(), Answer::Late(Ok(())), Answer::Never] {
        let (outcome, cancels) = created_then_triggered(answer);
        assert!(matches!(outcome, Err(Stop::Cancelled)));
        assert_eq!(cancels, 1);
    }
}

#[test]
fn given_a_create_that_ended_removed_when_the_view_triggers_then_the_call_stops_with_no_cancel() {
    let (outcome, cancels) = created_then_triggered(Answer::Now(Err(Removal::LeftUnsent)));
    assert!(matches!(outcome, Err(Stop::Cancelled)));
    assert_eq!(cancels, 0);
}

#[test]
fn given_a_view_not_triggered_when_the_create_was_taken_then_the_call_goes_on_with_no_cancel() {
    let (code, script) = code(Answer::delivered());
    let form = PayloadForm::Kept;
    let token = CancelToken::new();
    let created = code
        .hand_create(Uuid::from_u128(2), form, &token.view())
        .ok()
        .expect("a create");
    assert!(
        code.stop_if_cancelled(Uuid::from_u128(2), created, &token.view())
            .is_ok()
    );
    assert!(
        !script
            .calls()
            .iter()
            .any(|call| matches!(call, MemoryCall::Cancel { .. }))
    );
}
