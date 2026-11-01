use br_graph_executor::{Checkpoint, RunFailure};
use br_runner_jobs::FailureKind;

use super::{first_found, found_of};

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
