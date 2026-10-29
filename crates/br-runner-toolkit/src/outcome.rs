use br_graph_contract::Fields;
use br_graph_executor::Outcome;

use crate::failure::{FailureMapping, FailureReport, executor_failure, map_failure};

#[derive(Debug, Clone, PartialEq)]
pub enum Terminal {
    Completed { hand_over_state: bool },
    Failed(FailureReport),
    Cancelled { failures: Vec<String> },
}

pub fn terminal_for<F: Fields>(
    outcome: &Outcome<F>,
    can_restart: bool,
    inbox_agent_registered: bool,
    mapping: Option<&FailureMapping>,
) -> Terminal {
    match outcome {
        Outcome::RunEnded { .. } => Terminal::Completed {
            hand_over_state: can_restart && inbox_agent_registered,
        },
        Outcome::RunFailed { failures, .. } => match failures.first() {
            Some(first) => {
                let (code, text, cause) = executor_failure(first);
                Terminal::Failed(map_failure(code, text, cause, mapping))
            }
            None => Terminal::Failed(map_failure(
                "run_failed",
                "the run failed with no failure listed".to_owned(),
                None,
                mapping,
            )),
        },
        Outcome::RunCancelled { failures, .. } => Terminal::Cancelled {
            failures: failures.iter().map(ToString::to_string).collect(),
        },
        _ => Terminal::Failed(FailureReport::permanent(
            "run_failed",
            "the executor gave an outcome the toolkit does not know",
        )),
    }
}
