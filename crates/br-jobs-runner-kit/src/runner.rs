use std::future::Future;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::context::RunContext;

pub use contract_jobs::runner::FailureReport;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    Completed,
    Failed {
        report: FailureReport,
        retry_after: Option<Duration>,
    },
}

impl RunOutcome {
    pub fn failed(reason_code: impl Into<String>) -> Self {
        Self::Failed {
            report: FailureReport {
                kind: None,
                reason_code: reason_code.into(),
                params: Value::Null,
                diagnostic: Value::Null,
            },
            retry_after: None,
        }
    }

    pub fn failed_retry_after(reason_code: impl Into<String>, retry_after: Duration) -> Self {
        match Self::failed(reason_code) {
            Self::Failed { report, .. } => Self::Failed {
                report,
                retry_after: Some(retry_after),
            },
            Self::Completed => unreachable!("failed() always builds a failure"),
        }
    }
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
    use crate::facts::RunFact;
    use uuid::Uuid;

    struct EchoRunner;

    impl Runner for EchoRunner {
        type Payload = String;

        async fn execute(&self, ctx: RunContext, payload: Self::Payload) -> RunOutcome {
            ctx.log(payload.clone());
            if payload == "boom" {
                return RunOutcome::failed_retry_after("asked_to_fail", Duration::from_secs(60));
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
        let RunFact::Log(line) = facts.try_recv().expect("a log fact was emitted") else {
            panic!("expected a log fact");
        };
        assert_eq!((line.run_id, line.message.as_str()), (run_id, "hello"));
    }

    #[tokio::test]
    async fn given_failing_run_when_executing_then_failure_carries_code_and_retry_hint() {
        let (ctx, _facts, _cancel) = RunContext::stub(Uuid::now_v7());

        let outcome = EchoRunner.execute(ctx, "boom".to_owned()).await;

        let RunOutcome::Failed {
            report,
            retry_after,
        } = outcome
        else {
            panic!("expected a failure");
        };
        assert_eq!(report.reason_code, "asked_to_fail");
        assert_eq!(retry_after, Some(Duration::from_secs(60)));
    }
}
