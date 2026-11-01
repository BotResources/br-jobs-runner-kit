use br_graph_executor::{Checkpoint, RunFailure};
use br_runner_jobs::FailureKind;

use super::{first_found, found_of};
use crate::unit_support::Steps;

fn checkpoint() -> Checkpoint {
    serde_json::from_value(serde_json::json!({
        "state": "{}",
        "structure": {
            "version": "1",
            "entries": ["work"],
            "nodes": [{ "name": "work", "kind": "programmatic", "subkind": "worker" }],
            "edges": [{ "name": "work", "source": "work", "targets": ["END"] }]
        },
        "nodes": []
    }))
    .expect("a checkpoint")
}

fn node() -> String {
    "work".to_owned()
}

fn reason() -> String {
    "the reason".to_owned()
}

#[test]
fn given_each_failure_of_the_executor_when_it_is_found_then_the_reason_code_is_its_name_the_kind_is_permanent_and_the_text_is_its_reason()
 {
    let table = [
        (
            RunFailure::NodeFailed {
                node: node(),
                reason: reason(),
                cause: None,
            },
            "node_failed",
        ),
        (
            RunFailure::NodeRefusedInput {
                node: node(),
                reason: reason(),
            },
            "node_refused_an_input",
        ),
        (
            RunFailure::NodeCouldNotStart {
                node: node(),
                reason: reason(),
                cause: None,
            },
            "node_could_not_start",
        ),
        (
            RunFailure::NodeVanished {
                node: node(),
                reason: reason(),
            },
            "node_vanished",
        ),
        (
            RunFailure::UpdateRefused {
                node: node(),
                reason: reason(),
            },
            "update_refused",
        ),
        (
            RunFailure::EdgeFailed {
                node: node(),
                reason: reason(),
                cause: None,
            },
            "edge_failed",
        ),
        (
            RunFailure::CheckpointNotBuilt { reason: reason() },
            "checkpoint_not_built",
        ),
        (
            RunFailure::SinkRefused {
                reason: reason(),
                checkpoint: checkpoint(),
            },
            "sink_refused",
        ),
    ];
    for (failure, code) in table {
        let found = found_of(failure);
        assert_eq!(found.reason_code, code);
        assert_eq!(found.text, reason(), "{code}");
        assert_eq!(found.kind, FailureKind::Permanent, "{code}");
        assert!(found.cause.is_none(), "{code}");
    }
}

#[test]
fn given_a_failed_outcome_with_no_failure_when_it_is_found_then_the_reason_code_is_run_failed_and_permanent()
 {
    let found = first_found(Vec::new());
    assert_eq!(found.reason_code, "run_failed");
    assert_eq!(found.kind, FailureKind::Permanent);
}

#[test]
fn given_several_failures_when_they_are_found_then_the_first_one_is_the_reason() {
    let found = first_found(vec![
        RunFailure::NodeVanished {
            node: node(),
            reason: reason(),
        },
        RunFailure::UpdateRefused {
            node: node(),
            reason: reason(),
        },
    ]);
    assert_eq!(found.reason_code, "node_vanished");
}

fn run_record() -> std::sync::Arc<crate::record::RunRecord> {
    crate::record::RunRecord::new(
        uuid::Uuid::from_u128(1),
        uuid::Uuid::from_u128(2),
        br_graph_contract::CancelToken::new().handle().clone(),
    )
}

#[test]
fn given_an_executor_that_returned_an_outcome_when_the_start_is_followed_up_then_the_run_expects_the_end_of_the_observations()
 {
    let record = run_record();
    record.set_executor_running(true);
    let outcome = br_graph_executor::Outcome::<Steps>::RunCancelled {
        failures: Vec::new(),
        checkpoint: checkpoint(),
    };
    let ending = super::after_start(&record, None, Ok(outcome));
    assert!(matches!(ending, super::Ending::Cancelled));
    assert!(record.observations_expected());
    assert!(!record.executor_running());
}

#[test]
fn given_a_start_the_executor_refused_when_the_start_is_followed_up_then_the_run_does_not_expect_observations_and_fails_with_start_refused()
 {
    let record = run_record();
    record.set_executor_running(true);
    let ending =
        super::after_start::<Steps>(&record, None, Err(br_graph_executor::StartError::NoNodes));
    assert!(!record.observations_expected());
    assert!(!record.executor_running());
    match ending {
        super::Ending::Failed(failed) => assert_eq!(failed.reason_code, "start_refused"),
        _ => panic!("a failed ending was expected"),
    }
}
