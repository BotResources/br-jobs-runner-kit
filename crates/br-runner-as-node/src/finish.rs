use br_graph_contract::{CancelView, Event, Fields};
use br_runner_jobs::CallerHandle;
use uuid::Uuid;

use crate::outcome::Outcome;
use crate::signals::Stop;

pub(crate) fn hand_over_unless_cancelled<F: Fields>(
    cancel: &CancelView,
    caller: &CallerHandle,
    child: Uuid,
    outcome: &Outcome,
) -> Result<(), Stop<F>> {
    if cancel.is_triggered() {
        return Err(Stop::Event(Event::Cancelled));
    }
    if matches!(outcome, Outcome::Completed { .. }) {
        let _ = caller.finish(child, Uuid::now_v7());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use br_graph_contract::{CancelToken, Event};
    use br_runner_jobs::test_double::{MemoryCall, memory_caller};
    use uuid::Uuid;

    use super::hand_over_unless_cancelled;
    use crate::outcome::Outcome;
    use crate::signals::Stop;

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Flow;

    impl br_graph_contract::Fields for Flow {
        type Update = ();

        fn empty_update() {}

        fn validate(&self) -> Result<(), br_graph_contract::Reason> {
            Ok(())
        }

        fn apply(&mut self, _update: ()) -> Result<(), br_graph_contract::Reason> {
            Ok(())
        }
    }

    fn completed() -> Outcome {
        Outcome::Completed {
            inline: None,
            result_ref: None,
        }
    }

    fn finishes(calls: &[MemoryCall]) -> usize {
        calls
            .iter()
            .filter(|call| matches!(call, MemoryCall::Finish { .. }))
            .count()
    }

    #[test]
    fn given_the_cancel_view_triggered_after_the_end_when_the_finish_is_due_then_none_is_handed_over()
     {
        let (handle, script) = memory_caller();
        let token = CancelToken::new();
        token.handle().trigger();

        let verdict = hand_over_unless_cancelled::<Flow>(
            &token.view(),
            &handle,
            Uuid::now_v7(),
            &completed(),
        );

        assert!(matches!(verdict, Err(Stop::Event(Event::Cancelled))));
        assert_eq!(finishes(&script.calls()), 0);
    }

    #[test]
    fn given_a_cancel_view_not_triggered_when_the_end_is_completed_then_one_finish_is_handed_over()
    {
        let (handle, script) = memory_caller();
        let token = CancelToken::new();

        let verdict = hand_over_unless_cancelled::<Flow>(
            &token.view(),
            &handle,
            Uuid::now_v7(),
            &completed(),
        );

        assert!(verdict.is_ok());
        assert_eq!(finishes(&script.calls()), 1);
    }

    #[test]
    fn given_a_cancel_view_not_triggered_when_the_end_is_not_completed_then_no_finish_is_handed_over()
     {
        let (handle, script) = memory_caller();
        let token = CancelToken::new();
        let failed = Outcome::Failed {
            report: None,
            message: "x".to_owned(),
        };

        let verdict =
            hand_over_unless_cancelled::<Flow>(&token.view(), &handle, Uuid::now_v7(), &failed);

        assert!(verdict.is_ok());
        assert_eq!(finishes(&script.calls()), 0);
    }
}
