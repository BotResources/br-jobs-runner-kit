use std::future::Future;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::context::RunContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureReport {
    pub reason: String,
    pub retry_after: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    Completed,
    Failed(FailureReport),
}

pub trait Runner: Send + Sync + 'static {
    type Payload: DeserializeOwned + Send + 'static;

    fn execute(
        &self,
        ctx: RunContext,
        payload: Self::Payload,
    ) -> impl Future<Output = RunOutcome> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct EchoRunner;

    impl Runner for EchoRunner {
        type Payload = String;

        async fn execute(&self, ctx: RunContext, payload: Self::Payload) -> RunOutcome {
            ctx.log(payload.clone());
            if payload == "boom" {
                return RunOutcome::Failed(FailureReport {
                    reason: "asked to fail".to_owned(),
                    retry_after: Some(Duration::from_secs(60)),
                });
            }
            RunOutcome::Completed
        }
    }

    #[tokio::test]
    async fn given_runner_impl_when_executing_then_outcome_and_facts_flow() {
        let run_id = Uuid::now_v7();
        let (ctx, mut facts, _cancel) = RunContext::stub(run_id);

        let outcome = EchoRunner.execute(ctx, "hello".to_owned()).await;

        assert_eq!(outcome, RunOutcome::Completed);
        let fact = facts.try_recv().expect("a log fact was emitted");
        assert_eq!(
            fact,
            crate::facts::RunFact::Log {
                run_id,
                line: "hello".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn given_failing_run_when_executing_then_failure_report_carries_retry_hint() {
        let (ctx, _facts, _cancel) = RunContext::stub(Uuid::now_v7());

        let outcome = EchoRunner.execute(ctx, "boom".to_owned()).await;

        let RunOutcome::Failed(report) = outcome else {
            panic!("expected a failure");
        };
        assert_eq!(report.retry_after, Some(Duration::from_secs(60)));
    }
}
